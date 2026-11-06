//! /api/complex-sales и /api/complex-sale-templates — продажи под заказ с себестоимостью, предоплатой и статусами
use crate::{
    auth::AuthContext,
    error::{AppError, AppResult},
    ledger,
    routes::{
        finance::{add_tx, check_wallet, reverse_tx},
        products::cost_nodes,
        sales::check_store,
    },
    AppState,
};
use atlas_domain::{catalog::unit_cost, sale::{round2, stock_effects}};
use axum::{
    extract::{Path, Query, State},
    routing::{delete, get},
    Json, Router,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::{FromRow, PgPool, Postgres, Transaction};
use std::collections::HashMap;
use uuid::Uuid;

pub const STATUSES: [&str; 4] = ["new", "in_progress", "done", "cancelled"];

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list).post(create))
        .route("/{id}", get(one).put(update).delete(remove))
        .route("/{id}/payments", axum::routing::post(payment_add))
        .route("/{id}/payments/{payment_id}", delete(payment_remove))
        .route("/{id}/comments", axum::routing::post(comment_add))
}

pub fn templates_router() -> Router<AppState> {
    Router::new().route("/", get(tpl_list).post(tpl_create)).route("/{id}", get(tpl_one).put(tpl_update).delete(tpl_remove))
}

fn ok() -> Json<Value> {
    Json(json!({ "success": true }))
}

// ---------- представления ----------
#[derive(Serialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct ItemView {
    #[serde(skip)]
    pub sale_id: Uuid,
    pub id: Uuid,
    pub product_id: Option<Uuid>,
    pub name: String,
    pub qty: f64,
    pub cost: f64,
    pub extra_cost: f64,
}

#[derive(Serialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct PaymentView {
    #[serde(skip)]
    pub sale_id: Uuid,
    pub id: Uuid,
    pub wallet_id: Option<Uuid>,
    pub amount: f64,
    pub comment: String,
    pub paid_at: DateTime<Utc>,
}

#[derive(Serialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct UpdateView {
    #[serde(skip)]
    pub sale_id: Uuid,
    pub kind: String,
    pub text: String,
    pub user_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
}

#[derive(Serialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct ComplexSaleView {
    pub id: Uuid,
    pub store_id: Uuid,
    pub user_id: Uuid,
    pub client_id: Option<Uuid>,
    pub template_name: String,
    pub name: String,
    pub status: String,
    pub comment: String,
    pub final_price: f64,
    pub sale_date: DateTime<Utc>,
    /// Σ (кол-во × себестоимость + доп. расходы)
    pub expenses_total: f64,
    pub paid: f64,
    #[sqlx(skip)]
    pub expected_profit: f64,
    #[sqlx(skip)]
    pub remaining: f64,
    #[sqlx(skip)]
    pub items: Vec<ItemView>,
    #[sqlx(skip)]
    pub payments: Vec<PaymentView>,
    #[sqlx(skip)]
    pub updates: Vec<UpdateView>,
}

const SELECT_SALES: &str = "SELECT c.id, c.store_id, c.user_id, c.client_id, c.template_name, c.name, c.status, c.comment, \
     c.final_price::float8 AS final_price, c.sale_date, \
     (SELECT COALESCE(SUM(i.qty * i.cost + i.extra_cost),0) FROM complex_sale_items i WHERE i.sale_id = c.id)::float8 AS expenses_total, \
     (SELECT COALESCE(SUM(p.amount),0) FROM complex_sale_payments p WHERE p.sale_id = c.id)::float8 AS paid \
     FROM complex_sales c WHERE c.organization_id=$1 AND c.deleted_at IS NULL AND ($2::uuid IS NULL OR c.id=$2) \
       AND ($3::uuid IS NULL OR c.store_id=$3) AND ($4::timestamptz IS NULL OR c.sale_date >= $4) AND ($5::timestamptz IS NULL OR c.sale_date < $5) \
       AND (NOT $6 OR c.status IN ('new','in_progress')) AND ($7::uuid IS NULL OR c.client_id=$7) \
     ORDER BY c.sale_date DESC, c.created_at DESC LIMIT 500";

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ListQuery {
    pub store_id: Option<Uuid>,
    pub from: Option<DateTime<Utc>>,
    pub to: Option<DateTime<Utc>>,
    #[serde(default)]
    pub active_only: bool,
    pub client_id: Option<Uuid>,
}

async fn fetch(pool: &PgPool, org: Uuid, id: Option<Uuid>, q: &ListQuery) -> AppResult<Vec<ComplexSaleView>> {
    let mut rows = sqlx::query_as::<_, ComplexSaleView>(SELECT_SALES)
        .bind(org)
        .bind(id)
        .bind(q.store_id)
        .bind(q.from)
        .bind(q.to)
        .bind(q.active_only)
        .bind(q.client_id)
        .fetch_all(pool)
        .await?;
    let ids: Vec<Uuid> = rows.iter().map(|r| r.id).collect();
    let items = sqlx::query_as::<_, ItemView>(
        "SELECT sale_id, id, product_id, name, qty::float8 AS qty, cost::float8 AS cost, extra_cost::float8 AS extra_cost \
         FROM complex_sale_items WHERE sale_id = ANY($1) ORDER BY name, id",
    )
    .bind(&ids)
    .fetch_all(pool)
    .await?;
    let payments = sqlx::query_as::<_, PaymentView>(
        "SELECT sale_id, id, wallet_id, amount::float8 AS amount, comment, paid_at FROM complex_sale_payments \
         WHERE sale_id = ANY($1) ORDER BY paid_at, id",
    )
    .bind(&ids)
    .fetch_all(pool)
    .await?;
    let updates = sqlx::query_as::<_, UpdateView>(
        "SELECT sale_id, kind, text, user_id, created_at FROM complex_sale_updates WHERE sale_id = ANY($1) ORDER BY created_at, id",
    )
    .bind(&ids)
    .fetch_all(pool)
    .await?;
    let mut by_items: HashMap<Uuid, Vec<ItemView>> = HashMap::new();
    for i in items {
        by_items.entry(i.sale_id).or_default().push(i);
    }
    let mut by_pay: HashMap<Uuid, Vec<PaymentView>> = HashMap::new();
    for p in payments {
        by_pay.entry(p.sale_id).or_default().push(p);
    }
    let mut by_upd: HashMap<Uuid, Vec<UpdateView>> = HashMap::new();
    for u in updates {
        by_upd.entry(u.sale_id).or_default().push(u);
    }
    for r in &mut rows {
        r.items = by_items.remove(&r.id).unwrap_or_default();
        r.payments = by_pay.remove(&r.id).unwrap_or_default();
        r.updates = by_upd.remove(&r.id).unwrap_or_default();
        r.expected_profit = round2(r.final_price - r.expenses_total);
        r.remaining = round2(r.final_price - r.paid);
    }
    Ok(rows)
}

async fn list(State(s): State<AppState>, ctx: AuthContext, Query(q): Query<ListQuery>) -> AppResult<Json<Vec<ComplexSaleView>>> {
    ctx.require("complex_sale.view")?;
    Ok(Json(fetch(&s.pool, ctx.org_id, None, &q).await?))
}

async fn one(State(s): State<AppState>, ctx: AuthContext, Path(id): Path<Uuid>) -> AppResult<Json<ComplexSaleView>> {
    ctx.require("complex_sale.view")?;
    fetch(&s.pool, ctx.org_id, Some(id), &ListQuery::default())
        .await?
        .into_iter()
        .next()
        .map(Json)
        .ok_or_else(|| AppError::NotFound("Комплексная продажа не найдена".into()))
}

// ---------- создание / изменение ----------
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemDto {
    pub product_id: Option<Uuid>,
    pub name: Option<String>,
    pub qty: f64,
    pub cost: Option<f64>,
    pub extra_cost: Option<f64>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaymentDto {
    pub wallet_id: Option<Uuid>,
    pub amount: f64,
    pub comment: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaleDto {
    pub store_id: Uuid,
    pub client_id: Option<Uuid>,
    pub template_name: Option<String>,
    pub name: String,
    pub status: Option<String>,
    pub comment: Option<String>,
    #[serde(default)]
    pub final_price: f64,
    pub sale_date: Option<DateTime<Utc>>,
    #[serde(default)]
    pub items: Vec<ItemDto>,
    /// только при создании (предоплата)
    #[serde(default)]
    pub payments: Vec<PaymentDto>,
}

struct ResolvedItem {
    product_id: Option<Uuid>,
    name: String,
    qty: f64,
    cost: f64,
    extra: f64,
}

async fn validate(s: &AppState, ctx: &AuthContext, dto: &SaleDto) -> AppResult<Vec<ResolvedItem>> {
    check_store(&s.pool, ctx, dto.store_id).await?;
    if dto.name.trim().is_empty() {
        return Err(AppError::BadRequest("Введите название".into()));
    }
    if let Some(st) = &dto.status {
        if !STATUSES.contains(&st.as_str()) {
            return Err(AppError::BadRequest("Неизвестный статус".into()));
        }
    }
    if !(dto.final_price.is_finite() && dto.final_price >= 0.0) {
        return Err(AppError::BadRequest("Стоимость для клиента не может быть отрицательной".into()));
    }
    if let Some(c) = dto.client_id {
        let (n,): (i64,) = sqlx::query_as("SELECT count(*) FROM clients WHERE id=$1 AND organization_id=$2 AND deleted_at IS NULL")
            .bind(c)
            .bind(ctx.org_id)
            .fetch_one(&s.pool)
            .await?;
        if n == 0 {
            return Err(AppError::BadRequest("Клиент не найден".into()));
        }
    }
    let pids: Vec<Uuid> = dto.items.iter().filter_map(|i| i.product_id).collect();
    let prods: Vec<(Uuid, String, bool)> = sqlx::query_as(
        "SELECT id, name, is_service FROM products WHERE organization_id=$1 AND deleted_at IS NULL AND id = ANY($2)",
    )
    .bind(ctx.org_id)
    .bind(&pids)
    .fetch_all(&s.pool)
    .await?;
    let pmap: HashMap<Uuid, (String, bool)> = prods.into_iter().map(|p| (p.0, (p.1, p.2))).collect();
    let costs = cost_nodes(&s.pool, ctx.org_id).await?;
    let mut out = Vec::new();
    for it in &dto.items {
        if !(it.qty.is_finite() && it.qty > 0.0) {
            return Err(AppError::BadRequest("Количество должно быть больше нуля".into()));
        }
        let extra = it.extra_cost.unwrap_or(0.0);
        if !(extra.is_finite() && extra >= 0.0) || !it.cost.map_or(true, |c| c.is_finite() && c >= 0.0) {
            return Err(AppError::BadRequest("Себестоимость и доп. расходы не могут быть отрицательными".into()));
        }
        match it.product_id {
            Some(pid) => {
                let (pname, _) = pmap.get(&pid).ok_or_else(|| AppError::BadRequest("Товар не найден".into()))?;
                out.push(ResolvedItem {
                    product_id: Some(pid),
                    name: it.name.clone().filter(|n| !n.trim().is_empty()).unwrap_or_else(|| pname.clone()),
                    qty: it.qty,
                    cost: it.cost.unwrap_or_else(|| unit_cost(&costs, pid)),
                    extra,
                });
            }
            None => {
                let name = it.name.as_deref().map(str::trim).filter(|n| !n.is_empty());
                let name = name.ok_or_else(|| AppError::BadRequest("Введите название расхода".into()))?;
                out.push(ResolvedItem { product_id: None, name: name.to_string(), qty: it.qty, cost: it.cost.unwrap_or(0.0), extra });
            }
        }
    }
    Ok(out)
}

/// Списание материалов со склада (движение `complex_sale`).
async fn apply_stock(tx: &mut Transaction<'_, Postgres>, s: &AppState, ctx: &AuthContext, store_id: Uuid, sale_id: Uuid, items: &[ResolvedItem]) -> AppResult<()> {
    let snodes = ledger::stock_nodes(&s.pool, ctx.org_id).await?;
    for it in items {
        if let Some(pid) = it.product_id {
            for (eid, eq) in stock_effects(&snodes, pid, it.qty) {
                let after = ledger::apply(tx, ctx.org_id, store_id, eid, "complex_sale", -eq, Some(sale_id), ctx.user_id).await?;
                if after < -1e-9 && !ctx.can("sale.out_of_stock") {
                    return Err(AppError::BadRequest(format!("Недостаточно товара: {}", it.name)));
                }
            }
        }
    }
    Ok(())
}

async fn write_items(tx: &mut Transaction<'_, Postgres>, sale_id: Uuid, items: &[ResolvedItem]) -> AppResult<()> {
    sqlx::query("DELETE FROM complex_sale_items WHERE sale_id=$1").bind(sale_id).execute(&mut **tx).await?;
    for it in items {
        sqlx::query(
            "INSERT INTO complex_sale_items (id, sale_id, product_id, name, qty, cost, extra_cost) \
             VALUES ($1,$2,$3,$4,$5::float8,$6::float8,$7::float8)",
        )
        .bind(Uuid::new_v4())
        .bind(sale_id)
        .bind(it.product_id)
        .bind(&it.name)
        .bind(it.qty)
        .bind(it.cost)
        .bind(it.extra)
        .execute(&mut **tx)
        .await?;
    }
    Ok(())
}

async fn log_update(tx: &mut Transaction<'_, Postgres>, sale_id: Uuid, user_id: Uuid, kind: &str, text: &str) -> AppResult<()> {
    sqlx::query("INSERT INTO complex_sale_updates (id, sale_id, user_id, kind, text) VALUES ($1,$2,$3,$4,$5)")
        .bind(Uuid::new_v4())
        .bind(sale_id)
        .bind(user_id)
        .bind(kind)
        .bind(text)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

async fn create(State(s): State<AppState>, ctx: AuthContext, Json(dto): Json<SaleDto>) -> AppResult<Json<ComplexSaleView>> {
    ctx.require("complex_sale.view")?;
    let items = validate(&s, &ctx, &dto).await?;
    let paid_sum: f64 = dto.payments.iter().map(|p| p.amount).sum();
    if dto.payments.iter().any(|p| !(p.amount.is_finite() && p.amount > 0.0)) || paid_sum > dto.final_price + 0.005 {
        return Err(AppError::BadRequest("Оплаты должны быть больше нуля и не превышать стоимость".into()));
    }
    for p in &dto.payments {
        if let Some(w) = p.wallet_id {
            check_wallet(&s.pool, ctx.org_id, w, dto.store_id).await?;
        }
    }
    let status = dto.status.clone().unwrap_or_else(|| "new".into());
    let id = Uuid::new_v4();
    let mut tx = s.pool.begin().await?;
    sqlx::query(
        "INSERT INTO complex_sales (id, organization_id, store_id, user_id, client_id, template_name, name, status, comment, final_price, sale_date) \
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10::float8,COALESCE($11, now()))",
    )
    .bind(id)
    .bind(ctx.org_id)
    .bind(dto.store_id)
    .bind(ctx.user_id)
    .bind(dto.client_id)
    .bind(dto.template_name.clone().unwrap_or_default())
    .bind(dto.name.trim())
    .bind(&status)
    .bind(dto.comment.clone().unwrap_or_default())
    .bind(dto.final_price)
    .bind(dto.sale_date)
    .execute(&mut *tx)
    .await?;
    write_items(&mut tx, id, &items).await?;
    if status != "cancelled" {
        apply_stock(&mut tx, &s, &ctx, dto.store_id, id, &items).await?;
    }
    for p in &dto.payments {
        let pid = Uuid::new_v4();
        sqlx::query("INSERT INTO complex_sale_payments (id, sale_id, wallet_id, amount, comment, user_id) VALUES ($1,$2,$3,$4::float8,$5,$6)")
            .bind(pid)
            .bind(id)
            .bind(p.wallet_id)
            .bind(p.amount)
            .bind(p.comment.clone().unwrap_or_default())
            .bind(ctx.user_id)
            .execute(&mut *tx)
            .await?;
        if let Some(w) = p.wallet_id {
            add_tx(&mut tx, ctx.org_id, w, p.amount, "complex_sale_payment", Some(pid), "", ctx.user_id).await?;
        }
    }
    log_update(&mut tx, id, ctx.user_id, "status", &format!("Создана, статус: {status}")).await?;
    tx.commit().await?;
    one(State(s), ctx, Path(id)).await
}

async fn update(
    State(s): State<AppState>,
    ctx: AuthContext,
    Path(id): Path<Uuid>,
    Json(dto): Json<SaleDto>,
) -> AppResult<Json<ComplexSaleView>> {
    ctx.require("complex_sale.view")?;
    let items = validate(&s, &ctx, &dto).await?;
    let mut tx = s.pool.begin().await?;
    let cur: Option<(String, Uuid, f64)> = sqlx::query_as(
        "SELECT c.status, c.store_id, COALESCE((SELECT SUM(p.amount) FROM complex_sale_payments p WHERE p.sale_id = c.id),0)::float8 \
         FROM complex_sales c WHERE c.id=$1 AND c.organization_id=$2 AND c.deleted_at IS NULL FOR UPDATE",
    )
    .bind(id)
    .bind(ctx.org_id)
    .fetch_optional(&mut *tx)
    .await?;
    let (old_status, old_store, paid) = cur.ok_or_else(|| AppError::NotFound("Комплексная продажа не найдена".into()))?;
    if dto.final_price + 0.005 < paid {
        return Err(AppError::BadRequest("Стоимость меньше уже оплаченной суммы".into()));
    }
    let status = dto.status.clone().unwrap_or_else(|| old_status.clone());
    // изменение состава или статуса: сторно прежнего списания и новое списание
    ledger::reverse(&mut tx, ctx.org_id, old_store, id, "complex_sale", ctx.user_id).await?;
    sqlx::query(
        "UPDATE complex_sales SET store_id=$3, client_id=$4, template_name=COALESCE($5, template_name), name=$6, status=$7, \
         comment=COALESCE($8, comment), final_price=$9::float8, sale_date=COALESCE($10, sale_date) WHERE id=$1 AND organization_id=$2",
    )
    .bind(id)
    .bind(ctx.org_id)
    .bind(dto.store_id)
    .bind(dto.client_id)
    .bind(&dto.template_name)
    .bind(dto.name.trim())
    .bind(&status)
    .bind(&dto.comment)
    .bind(dto.final_price)
    .bind(dto.sale_date)
    .execute(&mut *tx)
    .await?;
    write_items(&mut tx, id, &items).await?;
    if status != "cancelled" {
        apply_stock(&mut tx, &s, &ctx, dto.store_id, id, &items).await?;
    }
    if status != old_status {
        log_update(&mut tx, id, ctx.user_id, "status", &format!("Статус: {old_status} → {status}")).await?;
    }
    tx.commit().await?;
    one(State(s), ctx, Path(id)).await
}

async fn remove(State(s): State<AppState>, ctx: AuthContext, Path(id): Path<Uuid>) -> AppResult<Json<Value>> {
    ctx.require("complex_sale.delete")?;
    let mut tx = s.pool.begin().await?;
    let row: Option<(Uuid,)> =
        sqlx::query_as("SELECT store_id FROM complex_sales WHERE id=$1 AND organization_id=$2 AND deleted_at IS NULL FOR UPDATE")
            .bind(id)
            .bind(ctx.org_id)
            .fetch_optional(&mut *tx)
            .await?;
    let (store_id,) = row.ok_or_else(|| AppError::NotFound("Комплексная продажа не найдена".into()))?;
    ledger::reverse(&mut tx, ctx.org_id, store_id, id, "complex_sale", ctx.user_id).await?;
    let pays: Vec<(Uuid,)> = sqlx::query_as("SELECT id FROM complex_sale_payments WHERE sale_id=$1").bind(id).fetch_all(&mut *tx).await?;
    for (pid,) in pays {
        reverse_tx(&mut tx, pid, "complex_sale_payment", ctx.user_id).await?;
    }
    sqlx::query("UPDATE complex_sales SET deleted_at=now() WHERE id=$1").bind(id).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(ok())
}

// ---------- оплаты и комментарии ----------
async fn payment_add(
    State(s): State<AppState>,
    ctx: AuthContext,
    Path(id): Path<Uuid>,
    Json(dto): Json<PaymentDto>,
) -> AppResult<Json<ComplexSaleView>> {
    ctx.require("complex_sale.view")?;
    if !(dto.amount.is_finite() && dto.amount > 0.0) {
        return Err(AppError::BadRequest("Сумма должна быть больше нуля".into()));
    }
    let mut tx = s.pool.begin().await?;
    let cur: Option<(Uuid, f64, f64)> = sqlx::query_as(
        "SELECT c.store_id, c.final_price::float8, COALESCE((SELECT SUM(p.amount) FROM complex_sale_payments p WHERE p.sale_id = c.id),0)::float8 \
         FROM complex_sales c WHERE c.id=$1 AND c.organization_id=$2 AND c.deleted_at IS NULL FOR UPDATE",
    )
    .bind(id)
    .bind(ctx.org_id)
    .fetch_optional(&mut *tx)
    .await?;
    let (store_id, price, paid) = cur.ok_or_else(|| AppError::NotFound("Комплексная продажа не найдена".into()))?;
    if paid + dto.amount > price + 0.005 {
        return Err(AppError::BadRequest("Оплата больше остатка к оплате".into()));
    }
    if let Some(w) = dto.wallet_id {
        check_wallet(&s.pool, ctx.org_id, w, store_id).await?;
    }
    let pid = Uuid::new_v4();
    sqlx::query("INSERT INTO complex_sale_payments (id, sale_id, wallet_id, amount, comment, user_id) VALUES ($1,$2,$3,$4::float8,$5,$6)")
        .bind(pid)
        .bind(id)
        .bind(dto.wallet_id)
        .bind(dto.amount)
        .bind(dto.comment.clone().unwrap_or_default())
        .bind(ctx.user_id)
        .execute(&mut *tx)
        .await?;
    if let Some(w) = dto.wallet_id {
        add_tx(&mut tx, ctx.org_id, w, dto.amount, "complex_sale_payment", Some(pid), "", ctx.user_id).await?;
    }
    tx.commit().await?;
    one(State(s), ctx, Path(id)).await
}

async fn payment_remove(State(s): State<AppState>, ctx: AuthContext, Path((id, pid)): Path<(Uuid, Uuid)>) -> AppResult<Json<ComplexSaleView>> {
    ctx.require("complex_sale.delete")?;
    let mut tx = s.pool.begin().await?;
    let res = sqlx::query(
        "DELETE FROM complex_sale_payments p USING complex_sales c WHERE p.id=$2 AND p.sale_id=$1 AND c.id=p.sale_id AND c.organization_id=$3",
    )
    .bind(id)
    .bind(pid)
    .bind(ctx.org_id)
    .execute(&mut *tx)
    .await?;
    if res.rows_affected() == 0 {
        return Err(AppError::NotFound("Оплата не найдена".into()));
    }
    reverse_tx(&mut tx, pid, "complex_sale_payment", ctx.user_id).await?;
    tx.commit().await?;
    one(State(s), ctx, Path(id)).await
}

#[derive(Deserialize)]
pub struct CommentDto {
    pub text: String,
}

async fn comment_add(
    State(s): State<AppState>,
    ctx: AuthContext,
    Path(id): Path<Uuid>,
    Json(dto): Json<CommentDto>,
) -> AppResult<Json<ComplexSaleView>> {
    ctx.require("complex_sale.view")?;
    if dto.text.trim().is_empty() {
        return Err(AppError::BadRequest("Введите комментарий".into()));
    }
    let (n,): (i64,) = sqlx::query_as("SELECT count(*) FROM complex_sales WHERE id=$1 AND organization_id=$2 AND deleted_at IS NULL")
        .bind(id)
        .bind(ctx.org_id)
        .fetch_one(&s.pool)
        .await?;
    if n == 0 {
        return Err(AppError::NotFound("Комплексная продажа не найдена".into()));
    }
    let mut tx = s.pool.begin().await?;
    log_update(&mut tx, id, ctx.user_id, "comment", dto.text.trim()).await?;
    tx.commit().await?;
    one(State(s), ctx, Path(id)).await
}

// =========================== шаблоны ===========================
#[derive(Serialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct TemplateView {
    pub id: Uuid,
    pub name: String,
    pub final_price: f64,
    pub items: Value,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TemplateDto {
    pub name: String,
    #[serde(default)]
    pub final_price: f64,
    #[serde(default)]
    pub items: Value,
}

fn check_template(dto: &TemplateDto) -> AppResult<Value> {
    if dto.name.trim().is_empty() {
        return Err(AppError::BadRequest("Введите название шаблона".into()));
    }
    if !(dto.final_price.is_finite() && dto.final_price >= 0.0) {
        return Err(AppError::BadRequest("Стоимость не может быть отрицательной".into()));
    }
    match &dto.items {
        Value::Null => Ok(json!([])),
        Value::Array(_) => Ok(dto.items.clone()),
        _ => Err(AppError::BadRequest("Список расходов должен быть массивом".into())),
    }
}

async fn tpl_list(State(s): State<AppState>, ctx: AuthContext) -> AppResult<Json<Vec<TemplateView>>> {
    ctx.require("complex_sale.view")?;
    Ok(Json(
        sqlx::query_as::<_, TemplateView>(
            "SELECT id, name, final_price::float8 AS final_price, items FROM complex_sale_templates \
             WHERE organization_id=$1 AND deleted_at IS NULL ORDER BY name",
        )
        .bind(ctx.org_id)
        .fetch_all(&s.pool)
        .await?,
    ))
}

async fn tpl_one(State(s): State<AppState>, ctx: AuthContext, Path(id): Path<Uuid>) -> AppResult<Json<TemplateView>> {
    ctx.require("complex_sale.view")?;
    sqlx::query_as::<_, TemplateView>(
        "SELECT id, name, final_price::float8 AS final_price, items FROM complex_sale_templates \
         WHERE organization_id=$1 AND id=$2 AND deleted_at IS NULL",
    )
    .bind(ctx.org_id)
    .bind(id)
    .fetch_optional(&s.pool)
    .await?
    .map(Json)
    .ok_or_else(|| AppError::NotFound("Шаблон не найден".into()))
}

async fn tpl_create(State(s): State<AppState>, ctx: AuthContext, Json(dto): Json<TemplateDto>) -> AppResult<Json<TemplateView>> {
    ctx.require("complex_sale.templates")?;
    let items = check_template(&dto)?;
    let id = Uuid::new_v4();
    sqlx::query("INSERT INTO complex_sale_templates (id, organization_id, name, final_price, items) VALUES ($1,$2,$3,$4::float8,$5)")
        .bind(id)
        .bind(ctx.org_id)
        .bind(dto.name.trim())
        .bind(dto.final_price)
        .bind(items)
        .execute(&s.pool)
        .await?;
    tpl_one(State(s), ctx, Path(id)).await
}

async fn tpl_update(
    State(s): State<AppState>,
    ctx: AuthContext,
    Path(id): Path<Uuid>,
    Json(dto): Json<TemplateDto>,
) -> AppResult<Json<TemplateView>> {
    ctx.require("complex_sale.templates")?;
    let items = check_template(&dto)?;
    let res = sqlx::query(
        "UPDATE complex_sale_templates SET name=$3, final_price=$4::float8, items=$5 WHERE id=$1 AND organization_id=$2 AND deleted_at IS NULL",
    )
    .bind(id)
    .bind(ctx.org_id)
    .bind(dto.name.trim())
    .bind(dto.final_price)
    .bind(items)
    .execute(&s.pool)
    .await?;
    if res.rows_affected() == 0 {
        return Err(AppError::NotFound("Шаблон не найден".into()));
    }
    tpl_one(State(s), ctx, Path(id)).await
}

async fn tpl_remove(State(s): State<AppState>, ctx: AuthContext, Path(id): Path<Uuid>) -> AppResult<Json<Value>> {
    ctx.require("complex_sale.templates")?;
    let res = sqlx::query("UPDATE complex_sale_templates SET deleted_at=now() WHERE id=$1 AND organization_id=$2 AND deleted_at IS NULL")
        .bind(id)
        .bind(ctx.org_id)
        .execute(&s.pool)
        .await?;
    if res.rows_affected() == 0 {
        return Err(AppError::NotFound("Шаблон не найден".into()));
    }
    Ok(ok())
}
