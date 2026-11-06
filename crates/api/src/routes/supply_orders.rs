//! /api/supply-orders — заказы поставщикам
use crate::{
    auth::AuthContext,
    error::{AppError, AppResult},
    routes::sales::check_store,
    AppState,
};
use axum::{
    extract::{Path, Query, State},
    routing::get,
    Json, Router,
};
use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::{FromRow, PgPool};
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

pub const STATUSES: [&str; 5] = ["draft", "ordered", "partially_received", "received", "cancelled"];

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list).post(create))
        .route("/suggest", get(suggest))
        .route("/{id}", get(one).put(update).delete(remove))
}

#[derive(Serialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct OrderItemView {
    #[serde(skip)]
    pub order_id: Uuid,
    pub product_id: Uuid,
    pub name: String,
    pub qty: f64,
    pub unit_price: f64,
}

#[derive(Serialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct OrderView {
    pub id: Uuid,
    pub store_id: Uuid,
    pub supplier_id: Option<Uuid>,
    pub user_id: Uuid,
    pub status: String,
    pub order_date: NaiveDate,
    pub comment: String,
    pub total: f64,
    #[sqlx(skip)]
    pub items: Vec<OrderItemView>,
}

const SELECT_ORDERS: &str = "SELECT o.id, o.store_id, o.supplier_id, o.user_id, o.status, o.order_date, o.comment, \
     (SELECT COALESCE(SUM(i.qty * i.unit_price), 0) FROM supply_order_items i WHERE i.order_id = o.id)::float8 AS total \
     FROM supply_orders o WHERE o.organization_id=$1 AND o.deleted_at IS NULL \
       AND ($2::uuid IS NULL OR o.id=$2) AND ($3::uuid IS NULL OR o.store_id=$3) \
       AND ($4::date IS NULL OR o.order_date >= $4) AND ($5::date IS NULL OR o.order_date <= $5) \
       AND (NOT $6 OR o.status IN ('draft','ordered','partially_received')) \
     ORDER BY o.order_date DESC, o.created_at DESC LIMIT 500";

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ListQuery {
    pub store_id: Option<Uuid>,
    pub from: Option<NaiveDate>,
    pub to: Option<NaiveDate>,
    #[serde(default)]
    pub active_only: bool,
}

async fn fetch(pool: &PgPool, org: Uuid, id: Option<Uuid>, q: &ListQuery) -> AppResult<Vec<OrderView>> {
    let mut rows = sqlx::query_as::<_, OrderView>(SELECT_ORDERS)
        .bind(org)
        .bind(id)
        .bind(q.store_id)
        .bind(q.from)
        .bind(q.to)
        .bind(q.active_only)
        .fetch_all(pool)
        .await?;
    let ids: Vec<Uuid> = rows.iter().map(|r| r.id).collect();
    let items = sqlx::query_as::<_, OrderItemView>(
        "SELECT i.order_id, i.product_id, p.name, i.qty::float8 AS qty, i.unit_price::float8 AS unit_price \
         FROM supply_order_items i JOIN products p ON p.id = i.product_id WHERE i.order_id = ANY($1) ORDER BY p.name",
    )
    .bind(&ids)
    .fetch_all(pool)
    .await?;
    let mut by: HashMap<Uuid, Vec<OrderItemView>> = HashMap::new();
    for i in items {
        by.entry(i.order_id).or_default().push(i);
    }
    for r in &mut rows {
        r.items = by.remove(&r.id).unwrap_or_default();
    }
    Ok(rows)
}

async fn list(State(s): State<AppState>, ctx: AuthContext, Query(q): Query<ListQuery>) -> AppResult<Json<Vec<OrderView>>> {
    ctx.require("stock.view")?;
    Ok(Json(fetch(&s.pool, ctx.org_id, None, &q).await?))
}

async fn one(State(s): State<AppState>, ctx: AuthContext, Path(id): Path<Uuid>) -> AppResult<Json<OrderView>> {
    ctx.require("stock.view")?;
    fetch(&s.pool, ctx.org_id, Some(id), &ListQuery::default())
        .await?
        .into_iter()
        .next()
        .map(Json)
        .ok_or_else(|| AppError::NotFound("Заказ не найден".into()))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemDto {
    pub product_id: Uuid,
    pub qty: f64,
    pub unit_price: Option<f64>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrderDto {
    pub store_id: Uuid,
    pub supplier_id: Option<Uuid>,
    pub status: Option<String>,
    pub order_date: Option<NaiveDate>,
    pub comment: Option<String>,
    pub items: Vec<ItemDto>,
}

async fn validate(s: &AppState, ctx: &AuthContext, dto: &OrderDto) -> AppResult<()> {
    check_store(&s.pool, ctx, dto.store_id).await?;
    if let Some(st) = &dto.status {
        if !STATUSES.contains(&st.as_str()) {
            return Err(AppError::BadRequest("Неизвестный статус заказа".into()));
        }
    }
    if dto.items.is_empty() {
        return Err(AppError::BadRequest("Добавьте товары в заказ".into()));
    }
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
    let ids: Vec<Uuid> = dto.items.iter().map(|i| i.product_id).collect();
    let (found,): (i64,) = sqlx::query_as(
        "SELECT count(*) FROM products WHERE organization_id=$1 AND deleted_at IS NULL AND NOT is_service AND id = ANY($2)",
    )
    .bind(ctx.org_id)
    .bind(&ids)
    .fetch_one(&s.pool)
    .await?;
    let mut seen = HashSet::new();
    if found as usize != ids.len() || !ids.iter().all(|i| seen.insert(*i)) {
        return Err(AppError::BadRequest("Товар не найден, указан дважды или является услугой".into()));
    }
    for i in &dto.items {
        if !(i.qty.is_finite() && i.qty > 0.0) || !i.unit_price.map_or(true, |p| p.is_finite() && p >= 0.0) {
            return Err(AppError::BadRequest("Некорректное количество или цена".into()));
        }
    }
    Ok(())
}

async fn write_items(tx: &mut sqlx::Transaction<'_, sqlx::Postgres>, id: Uuid, dto: &OrderDto) -> AppResult<()> {
    sqlx::query("DELETE FROM supply_order_items WHERE order_id=$1").bind(id).execute(&mut **tx).await?;
    for i in &dto.items {
        // цена по умолчанию — текущая закупочная
        sqlx::query(
            "INSERT INTO supply_order_items (order_id, product_id, qty, unit_price) \
             VALUES ($1,$2,$3::float8,COALESCE($4::float8, (SELECT purchase_price::float8 FROM products WHERE id=$2)))",
        )
        .bind(id)
        .bind(i.product_id)
        .bind(i.qty)
        .bind(i.unit_price)
        .execute(&mut **tx)
        .await?;
    }
    Ok(())
}

async fn create(State(s): State<AppState>, ctx: AuthContext, Json(dto): Json<OrderDto>) -> AppResult<Json<OrderView>> {
    ctx.require("stock.view")?;
    validate(&s, &ctx, &dto).await?;
    let id = Uuid::new_v4();
    let mut tx = s.pool.begin().await?;
    sqlx::query(
        "INSERT INTO supply_orders (id, organization_id, store_id, supplier_id, user_id, status, order_date, comment) \
         VALUES ($1,$2,$3,$4,$5,COALESCE($6,'draft'),COALESCE($7, current_date),$8)",
    )
    .bind(id)
    .bind(ctx.org_id)
    .bind(dto.store_id)
    .bind(dto.supplier_id)
    .bind(ctx.user_id)
    .bind(&dto.status)
    .bind(dto.order_date)
    .bind(dto.comment.clone().unwrap_or_default())
    .execute(&mut *tx)
    .await?;
    write_items(&mut tx, id, &dto).await?;
    tx.commit().await?;
    Ok(Json(fetch(&s.pool, ctx.org_id, Some(id), &ListQuery::default()).await?.remove(0)))
}

async fn update(
    State(s): State<AppState>,
    ctx: AuthContext,
    Path(id): Path<Uuid>,
    Json(dto): Json<OrderDto>,
) -> AppResult<Json<OrderView>> {
    ctx.require("stock.view")?;
    validate(&s, &ctx, &dto).await?;
    let mut tx = s.pool.begin().await?;
    let res = sqlx::query(
        "UPDATE supply_orders SET store_id=$3, supplier_id=$4, status=COALESCE($5,status), order_date=COALESCE($6,order_date), \
         comment=COALESCE($7,comment) WHERE id=$1 AND organization_id=$2 AND deleted_at IS NULL",
    )
    .bind(id)
    .bind(ctx.org_id)
    .bind(dto.store_id)
    .bind(dto.supplier_id)
    .bind(&dto.status)
    .bind(dto.order_date)
    .bind(&dto.comment)
    .execute(&mut *tx)
    .await?;
    if res.rows_affected() == 0 {
        return Err(AppError::NotFound("Заказ не найден".into()));
    }
    write_items(&mut tx, id, &dto).await?;
    tx.commit().await?;
    Ok(Json(fetch(&s.pool, ctx.org_id, Some(id), &ListQuery::default()).await?.remove(0)))
}

async fn remove(State(s): State<AppState>, ctx: AuthContext, Path(id): Path<Uuid>) -> AppResult<Json<Value>> {
    ctx.require("supply.delete")?;
    let res = sqlx::query("UPDATE supply_orders SET deleted_at=now() WHERE id=$1 AND organization_id=$2 AND deleted_at IS NULL")
        .bind(id)
        .bind(ctx.org_id)
        .execute(&s.pool)
        .await?;
    if res.rows_affected() == 0 {
        return Err(AppError::NotFound("Заказ не найден".into()));
    }
    Ok(Json(json!({ "success": true })))
}

// ---------- предложение к заказу ----------
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SuggestQuery {
    pub store_id: Uuid,
    pub supplier_id: Option<Uuid>,
    pub category_id: Option<Uuid>,
}

#[derive(Serialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct Suggestion {
    pub product_id: Uuid,
    pub name: String,
    pub current_qty: f64,
    pub threshold: f64,
    /// Дозаказ до двух порогов остатка (не меньше 1 шт.)
    pub suggested_qty: f64,
    pub unit_price: f64,
}

/// Товары с остатком не выше порога (порог 0 = 1) и предлагаемое количество на заказ.
async fn suggest(State(s): State<AppState>, ctx: AuthContext, Query(q): Query<SuggestQuery>) -> AppResult<Json<Vec<Suggestion>>> {
    ctx.require("stock.view")?;
    let rows = sqlx::query_as::<_, Suggestion>(
        "SELECT * FROM ( \
           SELECT p.id AS product_id, p.name, COALESCE(ss.qty,0)::float8 AS current_qty, \
                  (CASE WHEN p.stock_threshold > 0 THEN p.stock_threshold ELSE 1 END)::float8 AS threshold, \
                  GREATEST(1, ceil(2 * (CASE WHEN p.stock_threshold > 0 THEN p.stock_threshold ELSE 1 END) - COALESCE(ss.qty,0)))::float8 AS suggested_qty, \
                  p.purchase_price::float8 AS unit_price \
           FROM products p LEFT JOIN store_stock ss ON ss.product_id = p.id AND ss.store_id = $2 \
           WHERE p.organization_id=$1 AND p.deleted_at IS NULL AND NOT p.is_service AND NOT p.is_composite \
             AND ($3::uuid IS NULL OR p.supplier_id=$3) AND ($4::uuid IS NULL OR p.category_id=$4) \
         ) t WHERE current_qty <= threshold ORDER BY name",
    )
    .bind(ctx.org_id)
    .bind(q.store_id)
    .bind(q.supplier_id)
    .bind(q.category_id)
    .fetch_all(&s.pool)
    .await?;
    Ok(Json(rows))
}
