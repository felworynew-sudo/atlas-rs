//! Складские документы: /api/supplies (приёмки), /api/writeoffs (списания), /api/revisions (ревизии)
use crate::{
    auth::AuthContext,
    error::{AppError, AppResult},
    ledger,
    routes::{products::cost_nodes, sales::check_store},
    AppState,
};
use atlas_domain::{catalog::unit_cost, sale::{round2, stock_effects}};
use axum::{
    extract::{Path, Query, State},
    routing::get,
    Json, Router,
};
use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::{FromRow, PgPool};
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

pub fn supplies_router() -> Router<AppState> {
    Router::new().route("/", get(supplies_list).post(supply_create)).route("/{id}", get(supply_one).delete(supply_remove))
}
pub fn writeoffs_router() -> Router<AppState> {
    Router::new().route("/", get(writeoffs_list).post(writeoff_create)).route("/{id}", get(writeoff_one).delete(writeoff_remove))
}
pub fn revisions_router() -> Router<AppState> {
    Router::new().route("/", get(revisions_list).post(revision_create)).route("/{id}", get(revision_one).delete(revision_remove))
}

fn ok() -> Json<Value> {
    Json(json!({ "success": true }))
}

/// (имя, услуга, составной, нецелые)
type ProductInfo = (String, bool, bool, bool);

async fn product_infos(pool: &PgPool, org: Uuid, ids: &[Uuid]) -> AppResult<HashMap<Uuid, ProductInfo>> {
    let rows: Vec<(Uuid, String, bool, bool, bool)> = sqlx::query_as(
        "SELECT id, name, is_service, is_composite, allow_fractional FROM products \
         WHERE organization_id=$1 AND deleted_at IS NULL AND id = ANY($2)",
    )
    .bind(org)
    .bind(ids)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(|(id, n, s, c, f)| (id, (n, s, c, f))).collect())
}

fn check_qty(qty: f64, info: &ProductInfo, allow_zero: bool) -> AppResult<()> {
    let bad = !qty.is_finite() || qty < 0.0 || (!allow_zero && qty == 0.0);
    if bad {
        return Err(AppError::BadRequest(format!("Некорректное количество «{}»", info.0)));
    }
    if !info.3 && qty.fract() != 0.0 {
        return Err(AppError::BadRequest(format!("Количество «{}» должно быть целым", info.0)));
    }
    Ok(())
}

// =========================== приёмки ===========================
#[derive(Serialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct SupplyItemView {
    #[serde(skip)]
    pub supply_id: Uuid,
    pub product_id: Uuid,
    pub name: String,
    pub qty: f64,
    pub price: f64,
}

#[derive(Serialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct SupplyView {
    pub id: Uuid,
    pub store_id: Uuid,
    pub supplier_id: Option<Uuid>,
    pub order_id: Option<Uuid>,
    pub user_id: Uuid,
    pub supply_date: NaiveDate,
    pub has_return: bool,
    pub prices_changed: bool,
    pub paid: bool,
    pub invoice_photo: Option<String>,
    pub comment: String,
    pub total: f64,
    #[sqlx(skip)]
    pub items: Vec<SupplyItemView>,
}

const SELECT_SUPPLIES: &str = "SELECT id, store_id, supplier_id, order_id, user_id, supply_date, has_return, prices_changed, paid, \
     invoice_photo, comment, total::float8 AS total FROM supplies \
     WHERE organization_id=$1 AND deleted_at IS NULL AND ($2::uuid IS NULL OR id=$2) AND ($3::uuid IS NULL OR store_id=$3) \
       AND ($4::date IS NULL OR supply_date >= $4) AND ($5::date IS NULL OR supply_date <= $5) \
       AND ($6::uuid IS NULL OR supplier_id=$6) ORDER BY supply_date DESC, created_at DESC LIMIT 500";

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SupplyQuery {
    pub store_id: Option<Uuid>,
    pub from: Option<NaiveDate>,
    pub to: Option<NaiveDate>,
    pub supplier_id: Option<Uuid>,
}

pub async fn fetch_supplies(pool: &PgPool, org: Uuid, id: Option<Uuid>, q: &SupplyQuery) -> AppResult<Vec<SupplyView>> {
    let mut rows = sqlx::query_as::<_, SupplyView>(SELECT_SUPPLIES)
        .bind(org)
        .bind(id)
        .bind(q.store_id)
        .bind(q.from)
        .bind(q.to)
        .bind(q.supplier_id)
        .fetch_all(pool)
        .await?;
    let ids: Vec<Uuid> = rows.iter().map(|r| r.id).collect();
    let items = sqlx::query_as::<_, SupplyItemView>(
        "SELECT i.supply_id, i.product_id, p.name, i.qty::float8 AS qty, i.price::float8 AS price \
         FROM supply_items i JOIN products p ON p.id = i.product_id WHERE i.supply_id = ANY($1) ORDER BY p.name",
    )
    .bind(&ids)
    .fetch_all(pool)
    .await?;
    let mut by: HashMap<Uuid, Vec<SupplyItemView>> = HashMap::new();
    for i in items {
        by.entry(i.supply_id).or_default().push(i);
    }
    for r in &mut rows {
        r.items = by.remove(&r.id).unwrap_or_default();
    }
    Ok(rows)
}

async fn supplies_list(State(s): State<AppState>, ctx: AuthContext, Query(q): Query<SupplyQuery>) -> AppResult<Json<Vec<SupplyView>>> {
    ctx.require("supply.history")?;
    Ok(Json(fetch_supplies(&s.pool, ctx.org_id, None, &q).await?))
}

async fn supply_one(State(s): State<AppState>, ctx: AuthContext, Path(id): Path<Uuid>) -> AppResult<Json<SupplyView>> {
    ctx.require("supply.history")?;
    fetch_supplies(&s.pool, ctx.org_id, Some(id), &SupplyQuery::default())
        .await?
        .into_iter()
        .next()
        .map(Json)
        .ok_or_else(|| AppError::NotFound("Приёмка не найдена".into()))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SupplyItemDto {
    pub product_id: Uuid,
    pub qty: f64,
    pub price: f64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SupplyDto {
    pub store_id: Uuid,
    pub supplier_id: Option<Uuid>,
    pub order_id: Option<Uuid>,
    pub supply_date: Option<NaiveDate>,
    #[serde(default)]
    pub has_return: bool,
    #[serde(default)]
    pub prices_changed: bool,
    #[serde(default)]
    pub paid: bool,
    pub invoice_photo: Option<String>,
    pub comment: Option<String>,
    /// счёт, с которого оплачена накладная (при `paid`)
    pub wallet_id: Option<Uuid>,
    pub items: Vec<SupplyItemDto>,
}

async fn supply_create(State(s): State<AppState>, ctx: AuthContext, Json(dto): Json<SupplyDto>) -> AppResult<Json<SupplyView>> {
    ctx.require("supply.history")?;
    if dto.items.is_empty() {
        return Err(AppError::BadRequest("Добавьте товары в приёмку".into()));
    }
    check_store(&s.pool, &ctx, dto.store_id).await?;
    if let Some(sup) = dto.supplier_id {
        let (n,): (i64,) = sqlx::query_as("SELECT count(*) FROM suppliers WHERE id=$1 AND organization_id=$2 AND deleted_at IS NULL")
            .bind(sup)
            .bind(ctx.org_id)
            .fetch_one(&s.pool)
            .await?;
        if n == 0 {
            return Err(AppError::BadRequest("Поставщик не найден".into()));
        }
    }
    if let Some(o) = dto.order_id {
        let (n,): (i64,) = sqlx::query_as(
            "SELECT count(*) FROM supply_orders WHERE id=$1 AND organization_id=$2 AND store_id=$3 AND deleted_at IS NULL AND status <> 'cancelled'",
        )
        .bind(o)
        .bind(ctx.org_id)
        .bind(dto.store_id)
        .fetch_one(&s.pool)
        .await?;
        if n == 0 {
            return Err(AppError::BadRequest("Заказ поставщику не найден".into()));
        }
    }
    let ids: Vec<Uuid> = dto.items.iter().map(|i| i.product_id).collect();
    let infos = product_infos(&s.pool, ctx.org_id, &ids).await?;
    let mut seen = HashSet::new();
    for it in &dto.items {
        let info = infos.get(&it.product_id).ok_or_else(|| AppError::BadRequest("Товар не найден".into()))?;
        if info.1 || info.2 {
            return Err(AppError::BadRequest(format!("«{}» нельзя принять: услуга или составной товар", info.0)));
        }
        check_qty(it.qty, info, false)?;
        if !(it.price.is_finite() && it.price >= 0.0) {
            return Err(AppError::BadRequest("Цена не может быть отрицательной".into()));
        }
        if !seen.insert(it.product_id) {
            return Err(AppError::BadRequest("Товар указан дважды".into()));
        }
    }
    let total = round2(dto.items.iter().map(|i| i.qty * i.price).sum());
    if let (true, Some(w)) = (dto.paid, dto.wallet_id) {
        crate::routes::finance::check_wallet(&s.pool, ctx.org_id, w, dto.store_id).await?;
    }

    let id = Uuid::new_v4();
    let mut tx = s.pool.begin().await?;
    sqlx::query(
        "INSERT INTO supplies (id, organization_id, store_id, supplier_id, order_id, user_id, supply_date, has_return, prices_changed, \
         paid, invoice_photo, comment, total, wallet_id) VALUES ($1,$2,$3,$4,$5,$6,COALESCE($7, current_date),$8,$9,$10,$11,$12,$13::float8,$14)",
    )
    .bind(id)
    .bind(ctx.org_id)
    .bind(dto.store_id)
    .bind(dto.supplier_id)
    .bind(dto.order_id)
    .bind(ctx.user_id)
    .bind(dto.supply_date)
    .bind(dto.has_return)
    .bind(dto.prices_changed)
    .bind(dto.paid)
    .bind(&dto.invoice_photo)
    .bind(dto.comment.unwrap_or_default())
    .bind(total)
    .bind(dto.wallet_id.filter(|_| dto.paid))
    .execute(&mut *tx)
    .await?;
    if let (true, Some(w)) = (dto.paid, dto.wallet_id) {
        // оплата накладной двигает деньги счёта; возврат поставщику — деньги приходят обратно
        let delta = if dto.has_return { total } else { -total };
        crate::routes::finance::add_tx(&mut tx, ctx.org_id, w, delta, "supply_payment", Some(id), "", ctx.user_id).await?;
    }
    for it in &dto.items {
        sqlx::query("INSERT INTO supply_items (id, supply_id, product_id, qty, price) VALUES ($1,$2,$3,$4::float8,$5::float8)")
            .bind(Uuid::new_v4())
            .bind(id)
            .bind(it.product_id)
            .bind(it.qty)
            .bind(it.price)
            .execute(&mut *tx)
            .await?;
        let delta = if dto.has_return { -it.qty } else { it.qty };
        let after = ledger::apply(&mut tx, ctx.org_id, dto.store_id, it.product_id, "supply", delta, Some(id), ctx.user_id).await?;
        if dto.has_return && after < -1e-9 {
            return Err(AppError::BadRequest(format!("Недостаточно товара для возврата поставщику: {}", infos[&it.product_id].0)));
        }
        if dto.prices_changed && !dto.has_return {
            sqlx::query("UPDATE products SET purchase_price=$2::float8 WHERE id=$1")
                .bind(it.product_id)
                .bind(it.price)
                .execute(&mut *tx)
                .await?;
        }
    }
    if let (Some(order_id), false) = (dto.order_id, dto.has_return) {
        refresh_order_status(&mut tx, order_id).await?;
    }
    tx.commit().await?;
    Ok(Json(fetch_supplies(&s.pool, ctx.org_id, Some(id), &SupplyQuery::default()).await?.remove(0)))
}

/// Статус заказа поставщику по фактически принятым количествам.
pub async fn refresh_order_status(tx: &mut sqlx::Transaction<'_, sqlx::Postgres>, order_id: Uuid) -> AppResult<()> {
    let (all,): (Option<bool>,) = sqlx::query_as(
        "SELECT bool_and(COALESCE(r.qty,0) >= i.qty) FROM supply_order_items i \
         LEFT JOIN (SELECT si.product_id, SUM(si.qty) AS qty FROM supply_items si \
                    JOIN supplies s ON s.id = si.supply_id AND s.deleted_at IS NULL AND NOT s.has_return \
                    WHERE s.order_id = $1 GROUP BY si.product_id) r ON r.product_id = i.product_id \
         WHERE i.order_id = $1",
    )
    .bind(order_id)
    .fetch_one(&mut **tx)
    .await?;
    let status = if all == Some(true) { "received" } else { "partially_received" };
    sqlx::query("UPDATE supply_orders SET status=$2 WHERE id=$1 AND status <> 'cancelled'")
        .bind(order_id)
        .bind(status)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

async fn supply_remove(State(s): State<AppState>, ctx: AuthContext, Path(id): Path<Uuid>) -> AppResult<Json<Value>> {
    ctx.require("supply.delete")?;
    let mut tx = s.pool.begin().await?;
    let row: Option<(Uuid, Option<Uuid>)> = sqlx::query_as(
        "SELECT store_id, order_id FROM supplies WHERE id=$1 AND organization_id=$2 AND deleted_at IS NULL FOR UPDATE",
    )
    .bind(id)
    .bind(ctx.org_id)
    .fetch_optional(&mut *tx)
    .await?;
    let (store_id, order_id) = row.ok_or_else(|| AppError::NotFound("Приёмка не найдена".into()))?;
    ledger::reverse(&mut tx, ctx.org_id, store_id, id, "supply", ctx.user_id).await?;
    crate::routes::finance::reverse_tx(&mut tx, id, "supply_payment", ctx.user_id).await?;
    sqlx::query("UPDATE supplies SET deleted_at=now() WHERE id=$1").bind(id).execute(&mut *tx).await?;
    if let Some(o) = order_id {
        refresh_order_status(&mut tx, o).await?;
    }
    tx.commit().await?;
    crate::events::log(&s.pool, ctx.org_id, Some(store_id), ctx.user_id, "supply_deleted", "Удалена приёмка").await;
    Ok(ok())
}

// =========================== списания ===========================
#[derive(Serialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct WriteoffView {
    pub id: Uuid,
    pub store_id: Uuid,
    pub user_id: Uuid,
    pub product_id: Uuid,
    pub product_name: String,
    pub qty: f64,
    pub reason_id: Uuid,
    pub reason_name: String,
    pub amount: f64,
    pub comment: String,
    pub writeoff_date: DateTime<Utc>,
}

const SELECT_WRITEOFFS: &str = "SELECT w.id, w.store_id, w.user_id, w.product_id, p.name AS product_name, w.qty::float8 AS qty, \
     w.reason_id, r.name AS reason_name, w.amount::float8 AS amount, w.comment, w.writeoff_date \
     FROM writeoffs w JOIN products p ON p.id = w.product_id JOIN list_items r ON r.id = w.reason_id \
     WHERE w.organization_id=$1 AND w.deleted_at IS NULL AND ($2::uuid IS NULL OR w.id=$2) AND ($3::uuid IS NULL OR w.store_id=$3) \
       AND ($4::timestamptz IS NULL OR w.writeoff_date >= $4) AND ($5::timestamptz IS NULL OR w.writeoff_date < $5) \
     ORDER BY w.writeoff_date DESC, w.created_at DESC LIMIT 500";

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct RangeQuery {
    pub store_id: Option<Uuid>,
    pub from: Option<DateTime<Utc>>,
    pub to: Option<DateTime<Utc>>,
}

async fn fetch_writeoffs(pool: &PgPool, org: Uuid, id: Option<Uuid>, q: &RangeQuery) -> AppResult<Vec<WriteoffView>> {
    Ok(sqlx::query_as::<_, WriteoffView>(SELECT_WRITEOFFS)
        .bind(org)
        .bind(id)
        .bind(q.store_id)
        .bind(q.from)
        .bind(q.to)
        .fetch_all(pool)
        .await?)
}

async fn writeoffs_list(State(s): State<AppState>, ctx: AuthContext, Query(q): Query<RangeQuery>) -> AppResult<Json<Vec<WriteoffView>>> {
    ctx.require("writeoff.history")?;
    Ok(Json(fetch_writeoffs(&s.pool, ctx.org_id, None, &q).await?))
}

async fn writeoff_one(State(s): State<AppState>, ctx: AuthContext, Path(id): Path<Uuid>) -> AppResult<Json<WriteoffView>> {
    ctx.require("writeoff.history")?;
    fetch_writeoffs(&s.pool, ctx.org_id, Some(id), &RangeQuery::default())
        .await?
        .into_iter()
        .next()
        .map(Json)
        .ok_or_else(|| AppError::NotFound("Списание не найдено".into()))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WriteoffDto {
    pub store_id: Uuid,
    pub product_id: Uuid,
    pub qty: f64,
    pub reason_id: Option<Uuid>,
    pub amount: Option<f64>,
    pub comment: Option<String>,
    pub writeoff_date: Option<DateTime<Utc>>,
}

async fn writeoff_create(State(s): State<AppState>, ctx: AuthContext, Json(dto): Json<WriteoffDto>) -> AppResult<Json<WriteoffView>> {
    ctx.require("writeoff.history")?;
    check_store(&s.pool, &ctx, dto.store_id).await?;
    let reason_id = dto.reason_id.ok_or_else(|| AppError::BadRequest("Выберите причину списания".into()))?;
    let (n,): (i64,) = sqlx::query_as(
        "SELECT count(*) FROM list_items WHERE id=$1 AND organization_id=$2 AND kind='writeoff_reasons' AND deleted_at IS NULL",
    )
    .bind(reason_id)
    .bind(ctx.org_id)
    .fetch_one(&s.pool)
    .await?;
    if n == 0 {
        return Err(AppError::BadRequest("Причина списания не найдена".into()));
    }
    let infos = product_infos(&s.pool, ctx.org_id, &[dto.product_id]).await?;
    let info = infos.get(&dto.product_id).ok_or_else(|| AppError::BadRequest("Товар не найден".into()))?;
    if info.1 {
        return Err(AppError::BadRequest("Услугу нельзя списать".into()));
    }
    check_qty(dto.qty, info, false)?;
    let costs = cost_nodes(&s.pool, ctx.org_id).await?;
    let amount = match dto.amount {
        Some(a) if a.is_finite() && a >= 0.0 => round2(a),
        Some(_) => return Err(AppError::BadRequest("Сумма не может быть отрицательной".into())),
        None => round2(dto.qty * unit_cost(&costs, dto.product_id)),
    };
    let snodes = ledger::stock_nodes(&s.pool, ctx.org_id).await?;

    let id = Uuid::new_v4();
    let mut tx = s.pool.begin().await?;
    sqlx::query(
        "INSERT INTO writeoffs (id, organization_id, store_id, user_id, product_id, qty, reason_id, amount, comment, writeoff_date) \
         VALUES ($1,$2,$3,$4,$5,$6::float8,$7,$8::float8,$9,COALESCE($10, now()))",
    )
    .bind(id)
    .bind(ctx.org_id)
    .bind(dto.store_id)
    .bind(ctx.user_id)
    .bind(dto.product_id)
    .bind(dto.qty)
    .bind(reason_id)
    .bind(amount)
    .bind(dto.comment.unwrap_or_default())
    .bind(dto.writeoff_date)
    .execute(&mut *tx)
    .await?;
    for (eid, eq) in stock_effects(&snodes, dto.product_id, dto.qty) {
        let after = ledger::apply(&mut tx, ctx.org_id, dto.store_id, eid, "writeoff", -eq, Some(id), ctx.user_id).await?;
        if after < -1e-9 && !ctx.can("sale.out_of_stock") {
            return Err(AppError::BadRequest("Недостаточно товара для списания".into()));
        }
    }
    tx.commit().await?;
    Ok(Json(fetch_writeoffs(&s.pool, ctx.org_id, Some(id), &RangeQuery::default()).await?.remove(0)))
}

async fn writeoff_remove(State(s): State<AppState>, ctx: AuthContext, Path(id): Path<Uuid>) -> AppResult<Json<Value>> {
    ctx.require("writeoff.delete")?;
    let mut tx = s.pool.begin().await?;
    let row: Option<(Uuid,)> = sqlx::query_as("SELECT store_id FROM writeoffs WHERE id=$1 AND organization_id=$2 AND deleted_at IS NULL FOR UPDATE")
        .bind(id)
        .bind(ctx.org_id)
        .fetch_optional(&mut *tx)
        .await?;
    let (store_id,) = row.ok_or_else(|| AppError::NotFound("Списание не найдено".into()))?;
    ledger::reverse(&mut tx, ctx.org_id, store_id, id, "writeoff", ctx.user_id).await?;
    sqlx::query("UPDATE writeoffs SET deleted_at=now() WHERE id=$1").bind(id).execute(&mut *tx).await?;
    tx.commit().await?;
    crate::events::log(&s.pool, ctx.org_id, Some(store_id), ctx.user_id, "writeoff_deleted", "Удалено списание").await;
    Ok(ok())
}

// =========================== ревизии ===========================
#[derive(Serialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct RevisionItemView {
    #[serde(skip)]
    pub revision_id: Uuid,
    pub product_id: Uuid,
    pub name: String,
    pub system_qty: f64,
    pub fact_qty: f64,
    pub diff_amount: f64,
}

#[derive(Serialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct RevisionView {
    pub id: Uuid,
    pub store_id: Uuid,
    pub category_id: Option<Uuid>,
    pub user_id: Uuid,
    pub comment: String,
    pub total_diff: f64,
    pub revision_date: DateTime<Utc>,
    #[sqlx(skip)]
    pub items: Vec<RevisionItemView>,
}

const SELECT_REVISIONS: &str = "SELECT id, store_id, category_id, user_id, comment, total_diff::float8 AS total_diff, revision_date \
     FROM revisions WHERE organization_id=$1 AND deleted_at IS NULL AND ($2::uuid IS NULL OR id=$2) AND ($3::uuid IS NULL OR store_id=$3) \
       AND ($4::timestamptz IS NULL OR revision_date >= $4) AND ($5::timestamptz IS NULL OR revision_date < $5) \
     ORDER BY revision_date DESC, created_at DESC LIMIT 500";

async fn fetch_revisions(pool: &PgPool, org: Uuid, id: Option<Uuid>, q: &RangeQuery) -> AppResult<Vec<RevisionView>> {
    let mut rows = sqlx::query_as::<_, RevisionView>(SELECT_REVISIONS)
        .bind(org)
        .bind(id)
        .bind(q.store_id)
        .bind(q.from)
        .bind(q.to)
        .fetch_all(pool)
        .await?;
    let ids: Vec<Uuid> = rows.iter().map(|r| r.id).collect();
    let items = sqlx::query_as::<_, RevisionItemView>(
        "SELECT i.revision_id, i.product_id, p.name, i.system_qty::float8 AS system_qty, i.fact_qty::float8 AS fact_qty, \
         i.diff_amount::float8 AS diff_amount FROM revision_items i JOIN products p ON p.id = i.product_id \
         WHERE i.revision_id = ANY($1) ORDER BY p.name",
    )
    .bind(&ids)
    .fetch_all(pool)
    .await?;
    let mut by: HashMap<Uuid, Vec<RevisionItemView>> = HashMap::new();
    for i in items {
        by.entry(i.revision_id).or_default().push(i);
    }
    for r in &mut rows {
        r.items = by.remove(&r.id).unwrap_or_default();
    }
    Ok(rows)
}

async fn revisions_list(State(s): State<AppState>, ctx: AuthContext, Query(q): Query<RangeQuery>) -> AppResult<Json<Vec<RevisionView>>> {
    ctx.require("revision.view")?;
    Ok(Json(fetch_revisions(&s.pool, ctx.org_id, None, &q).await?))
}

async fn revision_one(State(s): State<AppState>, ctx: AuthContext, Path(id): Path<Uuid>) -> AppResult<Json<RevisionView>> {
    ctx.require("revision.view")?;
    fetch_revisions(&s.pool, ctx.org_id, Some(id), &RangeQuery::default())
        .await?
        .into_iter()
        .next()
        .map(Json)
        .ok_or_else(|| AppError::NotFound("Ревизия не найдена".into()))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RevisionItemDto {
    pub product_id: Uuid,
    pub fact_qty: f64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RevisionDto {
    pub store_id: Uuid,
    pub category_id: Option<Uuid>,
    pub comment: Option<String>,
    pub items: Vec<RevisionItemDto>,
}

async fn revision_create(State(s): State<AppState>, ctx: AuthContext, Json(dto): Json<RevisionDto>) -> AppResult<Json<RevisionView>> {
    ctx.require("revision.view")?;
    if dto.items.is_empty() {
        return Err(AppError::BadRequest("Добавьте товары в ревизию".into()));
    }
    check_store(&s.pool, &ctx, dto.store_id).await?;
    if let Some(c) = dto.category_id {
        let (n,): (i64,) = sqlx::query_as("SELECT count(*) FROM product_categories WHERE id=$1 AND organization_id=$2 AND deleted_at IS NULL")
            .bind(c)
            .bind(ctx.org_id)
            .fetch_one(&s.pool)
            .await?;
        if n == 0 {
            return Err(AppError::BadRequest("Категория не найдена".into()));
        }
    }
    let ids: Vec<Uuid> = dto.items.iter().map(|i| i.product_id).collect();
    let infos = product_infos(&s.pool, ctx.org_id, &ids).await?;
    let mut seen = HashSet::new();
    for it in &dto.items {
        let info = infos.get(&it.product_id).ok_or_else(|| AppError::BadRequest("Товар не найден".into()))?;
        if info.1 {
            return Err(AppError::BadRequest(format!("У услуги «{}» нет остатков", info.0)));
        }
        check_qty(it.fact_qty, info, true)?;
        if !seen.insert(it.product_id) {
            return Err(AppError::BadRequest("Товар указан дважды".into()));
        }
    }
    let costs = cost_nodes(&s.pool, ctx.org_id).await?;

    let id = Uuid::new_v4();
    let mut tx = s.pool.begin().await?;
    sqlx::query("INSERT INTO revisions (id, organization_id, store_id, category_id, user_id, comment) VALUES ($1,$2,$3,$4,$5,$6)")
        .bind(id)
        .bind(ctx.org_id)
        .bind(dto.store_id)
        .bind(dto.category_id)
        .bind(ctx.user_id)
        .bind(dto.comment.unwrap_or_default())
        .execute(&mut *tx)
        .await?;
    let mut total = 0.0;
    for it in &dto.items {
        let system = ledger::lock_qty(&mut tx, dto.store_id, it.product_id).await?;
        let diff = ((it.fact_qty - system) * 10_000.0).round() / 10_000.0;
        let amount = round2(diff * unit_cost(&costs, it.product_id));
        total += amount;
        sqlx::query("INSERT INTO revision_items (revision_id, product_id, system_qty, fact_qty, diff_amount) VALUES ($1,$2,$3::float8,$4::float8,$5::float8)")
            .bind(id)
            .bind(it.product_id)
            .bind(system)
            .bind(it.fact_qty)
            .bind(amount)
            .execute(&mut *tx)
            .await?;
        if diff != 0.0 {
            ledger::apply(&mut tx, ctx.org_id, dto.store_id, it.product_id, "revision", diff, Some(id), ctx.user_id).await?;
        }
    }
    sqlx::query("UPDATE revisions SET total_diff=$2::float8 WHERE id=$1").bind(id).bind(round2(total)).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(Json(fetch_revisions(&s.pool, ctx.org_id, Some(id), &RangeQuery::default()).await?.remove(0)))
}

async fn revision_remove(State(s): State<AppState>, ctx: AuthContext, Path(id): Path<Uuid>) -> AppResult<Json<Value>> {
    ctx.require("revision.delete")?;
    let mut tx = s.pool.begin().await?;
    let row: Option<(Uuid,)> = sqlx::query_as("SELECT store_id FROM revisions WHERE id=$1 AND organization_id=$2 AND deleted_at IS NULL FOR UPDATE")
        .bind(id)
        .bind(ctx.org_id)
        .fetch_optional(&mut *tx)
        .await?;
    let (store_id,) = row.ok_or_else(|| AppError::NotFound("Ревизия не найдена".into()))?;
    ledger::reverse(&mut tx, ctx.org_id, store_id, id, "revision", ctx.user_id).await?;
    sqlx::query("UPDATE revisions SET deleted_at=now() WHERE id=$1").bind(id).execute(&mut *tx).await?;
    tx.commit().await?;
    crate::events::log(&s.pool, ctx.org_id, Some(store_id), ctx.user_id, "revision_deleted", "Удалена ревизия").await;
    Ok(ok())
}
