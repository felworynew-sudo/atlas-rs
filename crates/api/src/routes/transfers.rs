//! /api/transfers — перемещения между филиалами; /api/manufactures — производство по рецептуре
use crate::{
    auth::AuthContext,
    error::{AppError, AppResult},
    events, ledger,
    routes::{products::cost_nodes, sales::check_store},
    AppState,
};
use atlas_domain::{catalog::unit_cost, sale::{round2, stock_effects}};
use axum::{
    extract::{Path, Query, State},
    routing::get,
    Json, Router,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::{FromRow, PgPool};
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

pub fn transfers_router() -> Router<AppState> {
    Router::new().route("/", get(tr_list).post(tr_create)).route("/{id}", get(tr_one).delete(tr_remove))
}

pub fn manufactures_router() -> Router<AppState> {
    Router::new()
        .route("/", get(mf_list).post(mf_create))
        .route("/preview", get(mf_preview))
        .route("/{id}", get(mf_one).delete(mf_remove))
}

fn qty_text(v: f64) -> String {
    events::fmt(v)
}

/// (название, услуга, составной «во время продажи», нецелые, составной, тип производства)
struct Info {
    name: String,
    service: bool,
    fractional: bool,
    composite: bool,
    production: String,
}

async fn infos(pool: &PgPool, org: Uuid, ids: &[Uuid]) -> AppResult<HashMap<Uuid, Info>> {
    let rows: Vec<(Uuid, String, bool, bool, bool, String)> = sqlx::query_as(
        "SELECT id, name, is_service, allow_fractional, is_composite, production_type FROM products \
         WHERE organization_id=$1 AND deleted_at IS NULL AND id = ANY($2)",
    )
    .bind(org)
    .bind(ids)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(id, name, service, fractional, composite, production)| (id, Info { name, service, fractional, composite, production }))
        .collect())
}

fn check_qty(qty: f64, info: &Info) -> AppResult<()> {
    if !qty.is_finite() || qty <= 0.0 {
        return Err(AppError::BadRequest("Количество должно быть больше 0".into()));
    }
    if !info.fractional && qty.fract() != 0.0 {
        return Err(AppError::BadRequest(format!("Количество «{}» должно быть целым", info.name)));
    }
    Ok(())
}

// =========================== перемещения ===========================
#[derive(Serialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct TrItem {
    #[serde(skip)]
    pub transfer_id: Uuid,
    pub product_id: Uuid,
    pub name: String,
    pub qty: f64,
}

#[derive(Serialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct TransferView {
    pub id: Uuid,
    pub from_store_id: Uuid,
    pub to_store_id: Uuid,
    pub user_id: Uuid,
    pub transfer_date: DateTime<Utc>,
    pub comment: String,
    pub amount: f64,
    #[sqlx(skip)]
    pub items: Vec<TrItem>,
}

const SELECT_TRANSFERS: &str = "SELECT id, from_store_id, to_store_id, user_id, transfer_date, comment, amount::float8 AS amount FROM transfers \
     WHERE organization_id=$1 AND deleted_at IS NULL AND ($2::uuid IS NULL OR id=$2) \
       AND ($3::uuid IS NULL OR from_store_id=$3 OR to_store_id=$3) \
       AND ($4::timestamptz IS NULL OR transfer_date >= $4) AND ($5::timestamptz IS NULL OR transfer_date < $5) \
     ORDER BY transfer_date DESC, created_at DESC LIMIT 500";

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct RangeQuery {
    pub store_id: Option<Uuid>,
    pub from: Option<DateTime<Utc>>,
    pub to: Option<DateTime<Utc>>,
}

async fn tr_fetch(pool: &PgPool, org: Uuid, id: Option<Uuid>, q: &RangeQuery) -> AppResult<Vec<TransferView>> {
    let mut rows = sqlx::query_as::<_, TransferView>(SELECT_TRANSFERS)
        .bind(org)
        .bind(id)
        .bind(q.store_id)
        .bind(q.from)
        .bind(q.to)
        .fetch_all(pool)
        .await?;
    let ids: Vec<Uuid> = rows.iter().map(|r| r.id).collect();
    let items = sqlx::query_as::<_, TrItem>(
        "SELECT i.transfer_id, i.product_id, p.name, i.qty::float8 AS qty FROM transfer_items i JOIN products p ON p.id = i.product_id \
         WHERE i.transfer_id = ANY($1) ORDER BY p.name",
    )
    .bind(&ids)
    .fetch_all(pool)
    .await?;
    let mut by: HashMap<Uuid, Vec<TrItem>> = HashMap::new();
    for i in items {
        by.entry(i.transfer_id).or_default().push(i);
    }
    for r in &mut rows {
        r.items = by.remove(&r.id).unwrap_or_default();
    }
    Ok(rows)
}

async fn tr_list(State(s): State<AppState>, ctx: AuthContext, Query(q): Query<RangeQuery>) -> AppResult<Json<Vec<TransferView>>> {
    ctx.require("transfer.view")?;
    Ok(Json(tr_fetch(&s.pool, ctx.org_id, None, &q).await?))
}

async fn tr_one(State(s): State<AppState>, ctx: AuthContext, Path(id): Path<Uuid>) -> AppResult<Json<TransferView>> {
    ctx.require("transfer.view")?;
    tr_fetch(&s.pool, ctx.org_id, Some(id), &RangeQuery::default())
        .await?
        .into_iter()
        .next()
        .map(Json)
        .ok_or_else(|| AppError::NotFound("Перемещение не найдено".into()))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrItemDto {
    pub product_id: Uuid,
    pub qty: f64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransferDto {
    pub from_store_id: Uuid,
    pub to_store_id: Uuid,
    pub items: Vec<TrItemDto>,
    pub comment: Option<String>,
    pub transfer_date: Option<DateTime<Utc>>,
}

async fn tr_create(State(s): State<AppState>, ctx: AuthContext, Json(dto): Json<TransferDto>) -> AppResult<Json<TransferView>> {
    ctx.require("transfer.view")?;
    if dto.from_store_id == dto.to_store_id {
        return Err(AppError::BadRequest("Филиалы должны отличаться".into()));
    }
    if dto.items.is_empty() {
        return Err(AppError::BadRequest("Добавьте хотя бы один товар".into()));
    }
    check_store(&s.pool, &ctx, dto.from_store_id).await?;
    let (n,): (i64,) = sqlx::query_as("SELECT count(*) FROM stores WHERE id=$1 AND organization_id=$2 AND deleted_at IS NULL")
        .bind(dto.to_store_id)
        .bind(ctx.org_id)
        .fetch_one(&s.pool)
        .await?;
    if n == 0 {
        return Err(AppError::BadRequest("Выберите филиалы".into()));
    }
    let ids: Vec<Uuid> = dto.items.iter().map(|i| i.product_id).collect();
    let inf = infos(&s.pool, ctx.org_id, &ids).await?;
    let costs = cost_nodes(&s.pool, ctx.org_id).await?;
    let mut seen = HashSet::new();
    let mut amount = 0.0;
    for it in &dto.items {
        let i = inf.get(&it.product_id).ok_or_else(|| AppError::BadRequest("Товар не найден".into()))?;
        if i.service || (i.composite && i.production == "on_sale") {
            return Err(AppError::BadRequest(format!("«{}» нельзя переместить: у товара нет остатков", i.name)));
        }
        check_qty(it.qty, i)?;
        if !seen.insert(it.product_id) {
            return Err(AppError::BadRequest("Товар указан дважды".into()));
        }
        amount += it.qty * unit_cost(&costs, it.product_id);
    }

    let id = Uuid::new_v4();
    let mut tx = s.pool.begin().await?;
    sqlx::query(
        "INSERT INTO transfers (id, organization_id, from_store_id, to_store_id, user_id, transfer_date, comment, amount) \
         VALUES ($1,$2,$3,$4,$5,COALESCE($6, now()),$7,$8::float8)",
    )
    .bind(id)
    .bind(ctx.org_id)
    .bind(dto.from_store_id)
    .bind(dto.to_store_id)
    .bind(ctx.user_id)
    .bind(dto.transfer_date)
    .bind(dto.comment.unwrap_or_default())
    .bind(round2(amount))
    .execute(&mut *tx)
    .await?;
    for it in &dto.items {
        sqlx::query("INSERT INTO transfer_items (transfer_id, product_id, qty) VALUES ($1,$2,$3::float8)")
            .bind(id)
            .bind(it.product_id)
            .bind(it.qty)
            .execute(&mut *tx)
            .await?;
        let have = ledger::lock_qty(&mut tx, dto.from_store_id, it.product_id).await?;
        if have + 1e-9 < it.qty {
            return Err(AppError::BadRequest(format!(
                "Недостаточно товара «{}» в филиале (в наличии {})",
                inf[&it.product_id].name,
                qty_text(have)
            )));
        }
        ledger::apply(&mut tx, ctx.org_id, dto.from_store_id, it.product_id, "transfer", -it.qty, Some(id), ctx.user_id).await?;
        ledger::apply(&mut tx, ctx.org_id, dto.to_store_id, it.product_id, "transfer", it.qty, Some(id), ctx.user_id).await?;
    }
    tx.commit().await?;
    Ok(Json(tr_fetch(&s.pool, ctx.org_id, Some(id), &RangeQuery::default()).await?.remove(0)))
}

async fn tr_remove(State(s): State<AppState>, ctx: AuthContext, Path(id): Path<Uuid>) -> AppResult<Json<Value>> {
    ctx.require("transfer.delete")?;
    let mut tx = s.pool.begin().await?;
    let row: Option<(Uuid, Uuid)> = sqlx::query_as(
        "SELECT from_store_id, to_store_id FROM transfers WHERE id=$1 AND organization_id=$2 AND deleted_at IS NULL FOR UPDATE",
    )
    .bind(id)
    .bind(ctx.org_id)
    .fetch_optional(&mut *tx)
    .await?;
    let (from, to) = row.ok_or_else(|| AppError::NotFound("Перемещение не найдено".into()))?;
    ledger::reverse_in_store(&mut tx, ctx.org_id, from, id, "transfer", ctx.user_id).await?;
    ledger::reverse_in_store(&mut tx, ctx.org_id, to, id, "transfer", ctx.user_id).await?;
    sqlx::query("UPDATE transfers SET deleted_at=now() WHERE id=$1").bind(id).execute(&mut *tx).await?;
    tx.commit().await?;
    events::log(&s.pool, ctx.org_id, Some(from), ctx.user_id, "transfer_deleted", "Удалено перемещение").await;
    Ok(Json(json!({ "success": true })))
}

// =========================== производство ===========================
#[derive(Serialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct MfIngredient {
    #[serde(skip)]
    pub manufacture_id: Uuid,
    pub product_id: Uuid,
    pub name: String,
    pub qty: f64,
}

#[derive(Serialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct ManufactureView {
    pub id: Uuid,
    pub store_id: Uuid,
    pub product_id: Uuid,
    pub product_name: String,
    pub user_id: Uuid,
    pub qty: f64,
    pub batch: String,
    pub manufacture_date: DateTime<Utc>,
    pub cost_total: f64,
    #[sqlx(skip)]
    pub ingredients: Vec<MfIngredient>,
}

const SELECT_MF: &str = "SELECT m.id, m.store_id, m.product_id, p.name AS product_name, m.user_id, m.qty::float8 AS qty, m.batch, \
     m.manufacture_date, m.cost_total::float8 AS cost_total FROM manufactures m JOIN products p ON p.id = m.product_id \
     WHERE m.organization_id=$1 AND m.deleted_at IS NULL AND ($2::uuid IS NULL OR m.id=$2) AND ($3::uuid IS NULL OR m.store_id=$3) \
       AND ($4::timestamptz IS NULL OR m.manufacture_date >= $4) AND ($5::timestamptz IS NULL OR m.manufacture_date < $5) \
     ORDER BY m.manufacture_date DESC, m.created_at DESC LIMIT 500";

async fn mf_fetch(pool: &PgPool, org: Uuid, id: Option<Uuid>, q: &RangeQuery) -> AppResult<Vec<ManufactureView>> {
    let mut rows = sqlx::query_as::<_, ManufactureView>(SELECT_MF)
        .bind(org)
        .bind(id)
        .bind(q.store_id)
        .bind(q.from)
        .bind(q.to)
        .fetch_all(pool)
        .await?;
    let ids: Vec<Uuid> = rows.iter().map(|r| r.id).collect();
    let ing = sqlx::query_as::<_, MfIngredient>(
        "SELECT i.manufacture_id, i.product_id, p.name, i.qty::float8 AS qty FROM manufacture_ingredients i \
         JOIN products p ON p.id = i.product_id WHERE i.manufacture_id = ANY($1) ORDER BY p.name",
    )
    .bind(&ids)
    .fetch_all(pool)
    .await?;
    let mut by: HashMap<Uuid, Vec<MfIngredient>> = HashMap::new();
    for i in ing {
        by.entry(i.manufacture_id).or_default().push(i);
    }
    for r in &mut rows {
        r.ingredients = by.remove(&r.id).unwrap_or_default();
    }
    Ok(rows)
}

async fn mf_list(State(s): State<AppState>, ctx: AuthContext, Query(q): Query<RangeQuery>) -> AppResult<Json<Vec<ManufactureView>>> {
    ctx.require("manufacture.view")?;
    Ok(Json(mf_fetch(&s.pool, ctx.org_id, None, &q).await?))
}

async fn mf_one(State(s): State<AppState>, ctx: AuthContext, Path(id): Path<Uuid>) -> AppResult<Json<ManufactureView>> {
    ctx.require("manufacture.view")?;
    mf_fetch(&s.pool, ctx.org_id, Some(id), &RangeQuery::default())
        .await?
        .into_iter()
        .next()
        .map(Json)
        .ok_or_else(|| AppError::NotFound("Производство не найдено".into()))
}

/// Рецептура составного товара, умноженная на количество, и себестоимость партии.
async fn recipe(pool: &PgPool, org: Uuid, product: Uuid, qty: f64) -> AppResult<(Vec<(Uuid, f64)>, f64)> {
    let inf = infos(pool, org, &[product]).await?;
    let p = inf.get(&product).ok_or_else(|| AppError::NotFound("Товар не найден".into()))?;
    if !p.composite || p.production != "ahead" {
        return Err(AppError::BadRequest(format!("«{}» не производится заранее: выберите составной товар типа «произвести заранее»", p.name)));
    }
    let rows: Vec<(Uuid, f64)> = sqlx::query_as("SELECT ingredient_id, qty::float8 FROM product_ingredients WHERE product_id=$1")
        .bind(product)
        .fetch_all(pool)
        .await?;
    if rows.is_empty() {
        return Err(AppError::BadRequest("У товара нет состава".into()));
    }
    let scaled: Vec<(Uuid, f64)> = rows.into_iter().map(|(i, q)| (i, ((q * qty) * 10_000.0).round() / 10_000.0)).collect();
    let costs = cost_nodes(pool, org).await?;
    let extra: (f64,) = sqlx::query_as("SELECT extra_cost::float8 FROM products WHERE id=$1").bind(product).fetch_one(pool).await?;
    let cost = scaled.iter().map(|(i, q)| q * unit_cost(&costs, *i)).sum::<f64>() + extra.0 * qty;
    Ok((scaled, round2(cost)))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewQuery {
    pub product_id: Uuid,
    pub qty: f64,
}

/// Таблица ингредиентов формы производства и «Себестоимость партии».
async fn mf_preview(State(s): State<AppState>, ctx: AuthContext, Query(q): Query<PreviewQuery>) -> AppResult<Json<Value>> {
    ctx.require("manufacture.view")?;
    if !(q.qty.is_finite() && q.qty > 0.0) {
        return Err(AppError::BadRequest("Количество должно быть больше 0".into()));
    }
    let (ings, cost) = recipe(&s.pool, ctx.org_id, q.product_id, q.qty).await?;
    let ids: Vec<Uuid> = ings.iter().map(|i| i.0).collect();
    let inf = infos(&s.pool, ctx.org_id, &ids).await?;
    Ok(Json(json!({
        "ingredients": ings.iter().map(|(id, qty)| json!({"productId": id, "name": inf.get(id).map(|i| i.name.clone()), "qty": qty})).collect::<Vec<_>>(),
        "costTotal": cost,
    })))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IngredientDto {
    pub product_id: Uuid,
    pub qty: f64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManufactureDto {
    pub store_id: Uuid,
    pub product_id: Uuid,
    pub qty: f64,
    pub batch: Option<String>,
    pub manufacture_date: Option<DateTime<Utc>>,
    /// фактический расход ингредиентов (общее количество); по умолчанию — по рецепту
    pub ingredients: Option<Vec<IngredientDto>>,
}

pub async fn mf_create(State(s): State<AppState>, ctx: AuthContext, Json(dto): Json<ManufactureDto>) -> AppResult<Json<ManufactureView>> {
    ctx.require("manufacture.view")?;
    check_store(&s.pool, &ctx, dto.store_id).await?;
    let pinfo = infos(&s.pool, ctx.org_id, &[dto.product_id]).await?;
    let p = pinfo.get(&dto.product_id).ok_or_else(|| AppError::BadRequest("Товар не найден".into()))?;
    check_qty(dto.qty, p)?;
    let (mut ings, mut cost) = recipe(&s.pool, ctx.org_id, dto.product_id, dto.qty).await?;
    if let Some(custom) = &dto.ingredients {
        if custom.is_empty() {
            return Err(AppError::BadRequest("Добавьте ингредиенты".into()));
        }
        let ids: Vec<Uuid> = custom.iter().map(|c| c.product_id).collect();
        let cinf = infos(&s.pool, ctx.org_id, &ids).await?;
        let mut seen = HashSet::new();
        for c in custom {
            let i = cinf.get(&c.product_id).ok_or_else(|| AppError::BadRequest("Ингредиент не найден".into()))?;
            if i.service || !(c.qty.is_finite() && c.qty > 0.0) || !seen.insert(c.product_id) {
                return Err(AppError::BadRequest(format!("Некорректный ингредиент «{}»", i.name)));
            }
        }
        ings = custom.iter().map(|c| (c.product_id, c.qty)).collect();
        let costs = cost_nodes(&s.pool, ctx.org_id).await?;
        let extra: (f64,) = sqlx::query_as("SELECT extra_cost::float8 FROM products WHERE id=$1").bind(dto.product_id).fetch_one(&s.pool).await?;
        cost = round2(ings.iter().map(|(i, q)| q * unit_cost(&costs, *i)).sum::<f64>() + extra.0 * dto.qty);
    }
    let snodes = ledger::stock_nodes(&s.pool, ctx.org_id).await?;
    let id = Uuid::new_v4();
    let mut tx = s.pool.begin().await?;
    sqlx::query(
        "INSERT INTO manufactures (id, organization_id, store_id, product_id, user_id, qty, batch, manufacture_date, cost_total) \
         VALUES ($1,$2,$3,$4,$5,$6::float8,$7,COALESCE($8, now()),$9::float8)",
    )
    .bind(id)
    .bind(ctx.org_id)
    .bind(dto.store_id)
    .bind(dto.product_id)
    .bind(ctx.user_id)
    .bind(dto.qty)
    .bind(dto.batch.unwrap_or_default())
    .bind(dto.manufacture_date)
    .bind(cost)
    .execute(&mut *tx)
    .await?;
    for (ing, q) in &ings {
        sqlx::query("INSERT INTO manufacture_ingredients (manufacture_id, product_id, qty) VALUES ($1,$2,$3::float8)")
            .bind(id)
            .bind(ing)
            .bind(q)
            .execute(&mut *tx)
            .await?;
        for (eid, eq) in stock_effects(&snodes, *ing, *q) {
            let after = ledger::apply(&mut tx, ctx.org_id, dto.store_id, eid, "manufacture", -eq, Some(id), ctx.user_id).await?;
            if after < -1e-9 && !ctx.can("sale.out_of_stock") {
                let (name,): (String,) = sqlx::query_as("SELECT name FROM products WHERE id=$1").bind(eid).fetch_one(&mut *tx).await?;
                return Err(AppError::BadRequest(format!("Недостаточно ингредиента: {name}")));
            }
        }
    }
    ledger::apply(&mut tx, ctx.org_id, dto.store_id, dto.product_id, "manufacture", dto.qty, Some(id), ctx.user_id).await?;
    tx.commit().await?;
    Ok(Json(mf_fetch(&s.pool, ctx.org_id, Some(id), &RangeQuery::default()).await?.remove(0)))
}

async fn mf_remove(State(s): State<AppState>, ctx: AuthContext, Path(id): Path<Uuid>) -> AppResult<Json<Value>> {
    ctx.require("manufacture.delete")?;
    let mut tx = s.pool.begin().await?;
    let row: Option<(Uuid,)> =
        sqlx::query_as("SELECT store_id FROM manufactures WHERE id=$1 AND organization_id=$2 AND deleted_at IS NULL FOR UPDATE")
            .bind(id)
            .bind(ctx.org_id)
            .fetch_optional(&mut *tx)
            .await?;
    let (store,) = row.ok_or_else(|| AppError::NotFound("Производство не найдено".into()))?;
    ledger::reverse(&mut tx, ctx.org_id, store, id, "manufacture", ctx.user_id).await?;
    sqlx::query("UPDATE manufactures SET deleted_at=now() WHERE id=$1").bind(id).execute(&mut *tx).await?;
    tx.commit().await?;
    events::log(&s.pool, ctx.org_id, Some(store), ctx.user_id, "manufacture_deleted", "Удалено производство").await;
    Ok(Json(json!({ "success": true })))
}
