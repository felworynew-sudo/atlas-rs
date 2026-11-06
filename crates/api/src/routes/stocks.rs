//! /api/stocks — остатки по филиалу и история движения товара
use crate::{
    auth::AuthContext,
    error::{AppError, AppResult},
    AppState,
};
use axum::{
    extract::{Path, Query, State},
    routing::{get, put},
    Json, Router,
};
use crate::ledger;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

pub fn router() -> Router<AppState> {
    Router::new().route("/", get(list)).route("/{product_id}/movements", get(movements)).route("/{product_id}", put(set_qty))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetQtyDto {
    pub store_id: Uuid,
    pub qty: f64,
}

/// «Количество товара в наличии» из карточки товара: ручная установка остатка через движение `manual`.
async fn set_qty(
    State(s): State<AppState>,
    ctx: AuthContext,
    Path(product_id): Path<Uuid>,
    Json(dto): Json<SetQtyDto>,
) -> AppResult<Json<serde_json::Value>> {
    ctx.require("product.view")?;
    if !dto.qty.is_finite() {
        return Err(AppError::BadRequest("Некорректное количество".into()));
    }
    let row: Option<(bool, bool)> = sqlx::query_as(
        "SELECT is_service, allow_fractional FROM products WHERE id=$1 AND organization_id=$2 AND deleted_at IS NULL",
    )
    .bind(product_id)
    .bind(ctx.org_id)
    .fetch_optional(&s.pool)
    .await?;
    let (service, fractional) = row.ok_or_else(|| AppError::NotFound("Товар не найден".into()))?;
    if service {
        return Err(AppError::BadRequest("У услуги нет остатков".into()));
    }
    if !fractional && dto.qty.fract() != 0.0 {
        return Err(AppError::BadRequest("Количество должно быть целым".into()));
    }
    let (n,): (i64,) = sqlx::query_as("SELECT count(*) FROM stores WHERE id=$1 AND organization_id=$2 AND deleted_at IS NULL")
        .bind(dto.store_id)
        .bind(ctx.org_id)
        .fetch_one(&s.pool)
        .await?;
    if n == 0 {
        return Err(AppError::BadRequest("Филиал не найден".into()));
    }
    let mut tx = s.pool.begin().await?;
    let (cur,): (f64,) = sqlx::query_as(
        "WITH ins AS (INSERT INTO store_stock (product_id, store_id, qty) VALUES ($1,$2,0) ON CONFLICT DO NOTHING) \
         SELECT COALESCE((SELECT qty FROM store_stock WHERE product_id=$1 AND store_id=$2 FOR UPDATE), 0)::float8",
    )
    .bind(product_id)
    .bind(dto.store_id)
    .fetch_one(&mut *tx)
    .await?;
    let delta = ((dto.qty - cur) * 10_000.0).round() / 10_000.0;
    if delta != 0.0 {
        ledger::apply(&mut tx, ctx.org_id, dto.store_id, product_id, "manual", delta, None, ctx.user_id).await?;
    }
    tx.commit().await?;
    Ok(Json(serde_json::json!({ "qty": dto.qty })))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoreQuery {
    pub store_id: Uuid,
}

#[derive(Serialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct StockRow {
    pub product_id: Uuid,
    pub name: String,
    pub category_id: Option<Uuid>,
    pub qty: f64,
    /// порог подсветки; 0 в карточке товара означает порог по умолчанию — 1
    pub threshold: f64,
    pub low: bool,
}

async fn list(State(s): State<AppState>, ctx: AuthContext, Query(q): Query<StoreQuery>) -> AppResult<Json<Vec<StockRow>>> {
    ctx.require("stock.view")?;
    let rows = sqlx::query_as::<_, StockRow>(
        "SELECT p.id AS product_id, p.name, p.category_id, COALESCE(ss.qty,0)::float8 AS qty, \
                CASE WHEN p.stock_threshold > 0 THEN p.stock_threshold ELSE 1 END::float8 AS threshold, \
                COALESCE(ss.qty,0) <= CASE WHEN p.stock_threshold > 0 THEN p.stock_threshold ELSE 1 END AS low \
         FROM products p LEFT JOIN store_stock ss ON ss.product_id = p.id AND ss.store_id = $2 \
         WHERE p.organization_id=$1 AND p.deleted_at IS NULL AND NOT p.is_service \
           AND NOT EXISTS (SELECT 1 FROM product_stores ps WHERE ps.product_id=p.id AND ps.store_id=$2 AND NOT ps.active) \
         ORDER BY p.name",
    )
    .bind(ctx.org_id)
    .bind(q.store_id)
    .fetch_all(&s.pool)
    .await?;
    Ok(Json(rows))
}

#[derive(Serialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct MovementRow {
    pub id: Uuid,
    pub kind: String,
    pub delta: f64,
    pub qty_before: f64,
    pub qty_after: f64,
    pub ref_id: Option<Uuid>,
    pub user_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
}

async fn movements(
    State(s): State<AppState>,
    ctx: AuthContext,
    Path(product_id): Path<Uuid>,
    Query(q): Query<StoreQuery>,
) -> AppResult<Json<Vec<MovementRow>>> {
    ctx.require("stock.view")?;
    let (n,): (i64,) = sqlx::query_as("SELECT count(*) FROM products WHERE id=$1 AND organization_id=$2")
        .bind(product_id)
        .bind(ctx.org_id)
        .fetch_one(&s.pool)
        .await?;
    if n == 0 {
        return Err(AppError::NotFound("Товар не найден".into()));
    }
    let rows = sqlx::query_as::<_, MovementRow>(
        "SELECT id, kind, delta::float8 AS delta, qty_before::float8 AS qty_before, (qty_before + delta)::float8 AS qty_after, \
                ref_id, user_id, created_at \
         FROM product_movements WHERE organization_id=$1 AND product_id=$2 AND store_id=$3 \
         ORDER BY created_at DESC, id LIMIT 200",
    )
    .bind(ctx.org_id)
    .bind(product_id)
    .bind(q.store_id)
    .fetch_all(&s.pool)
    .await?;
    Ok(Json(rows))
}
