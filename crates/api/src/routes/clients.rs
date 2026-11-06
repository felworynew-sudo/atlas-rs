//! /api/clients — клиенты и /api/saved-carts — отложенные чеки
use crate::{
    auth::AuthContext,
    error::{AppError, AppResult},
    AppState,
};
use axum::{
    extract::{Path, Query, State},
    routing::{delete, get},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::FromRow;
use uuid::Uuid;

pub fn router() -> Router<AppState> {
    Router::new().route("/", get(list).post(create)).route("/{id}", get(one).put(update).delete(remove))
}

pub fn saved_carts_router() -> Router<AppState> {
    Router::new().route("/", get(carts_list).post(carts_create)).route("/{id}", delete(carts_remove))
}

#[derive(Serialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct ClientView {
    pub id: Uuid,
    pub name: String,
    pub phone: String,
    pub comment: String,
    pub bonus_balance: f64,
    pub bonus_level: Option<f64>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientDto {
    pub name: String,
    pub phone: Option<String>,
    pub comment: Option<String>,
    pub bonus_level: Option<f64>,
}

#[derive(Deserialize)]
pub struct ListQuery {
    pub search: Option<String>,
}

/// Телефон: оставляем цифры и ведущий «+».
fn normalize_phone(p: &str) -> String {
    let t = p.trim();
    let digits: String = t.chars().filter(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() {
        String::new()
    } else if t.starts_with('+') {
        format!("+{digits}")
    } else {
        digits
    }
}

fn check(dto: &ClientDto) -> AppResult<()> {
    if dto.name.trim().is_empty() {
        return Err(AppError::BadRequest("Введите имя клиента".into()));
    }
    if let Some(l) = dto.bonus_level {
        if !(0.0..=100.0).contains(&l) {
            return Err(AppError::BadRequest("Уровень кэшбэка от 0 до 100%".into()));
        }
    }
    Ok(())
}

fn conflict(e: AppError) -> AppError {
    match e {
        AppError::Conflict(_) => AppError::Conflict("Клиент с таким телефоном уже есть".into()),
        other => other,
    }
}

async fn list(State(s): State<AppState>, ctx: AuthContext, Query(q): Query<ListQuery>) -> AppResult<Json<Vec<ClientView>>> {
    ctx.require("client.view")?;
    let search = q.search.map(|v| v.trim().to_string()).filter(|v| !v.is_empty());
    let rows = sqlx::query_as::<_, ClientView>(
        "SELECT id, name, phone, comment, bonus_balance::float8 AS bonus_balance, bonus_level::float8 AS bonus_level \
         FROM clients WHERE organization_id=$1 AND deleted_at IS NULL \
         AND ($2::text IS NULL OR name ILIKE '%' || $2 || '%' OR phone LIKE '%' || $2 || '%') ORDER BY name LIMIT 500",
    )
    .bind(ctx.org_id)
    .bind(search)
    .fetch_all(&s.pool)
    .await?;
    Ok(Json(rows))
}

async fn one(State(s): State<AppState>, ctx: AuthContext, Path(id): Path<Uuid>) -> AppResult<Json<ClientView>> {
    ctx.require("client.card")?;
    let row = sqlx::query_as::<_, ClientView>(
        "SELECT id, name, phone, comment, bonus_balance::float8 AS bonus_balance, bonus_level::float8 AS bonus_level \
         FROM clients WHERE organization_id=$1 AND deleted_at IS NULL AND id=$2",
    )
    .bind(ctx.org_id)
    .bind(id)
    .fetch_optional(&s.pool)
    .await?
    .ok_or_else(|| AppError::NotFound("Клиент не найден".into()))?;
    Ok(Json(row))
}

async fn create(State(s): State<AppState>, ctx: AuthContext, Json(dto): Json<ClientDto>) -> AppResult<Json<ClientView>> {
    ctx.require("client.view")?;
    check(&dto)?;
    let id = Uuid::new_v4();
    sqlx::query("INSERT INTO clients (id, organization_id, name, phone, comment, bonus_level) VALUES ($1,$2,$3,$4,$5,$6::float8)")
        .bind(id)
        .bind(ctx.org_id)
        .bind(dto.name.trim())
        .bind(normalize_phone(dto.phone.as_deref().unwrap_or("")))
        .bind(dto.comment.unwrap_or_default())
        .bind(dto.bonus_level)
        .execute(&s.pool)
        .await
        .map_err(|e| conflict(e.into()))?;
    one(State(s), ctx, Path(id)).await
}

async fn update(
    State(s): State<AppState>,
    ctx: AuthContext,
    Path(id): Path<Uuid>,
    Json(dto): Json<ClientDto>,
) -> AppResult<Json<ClientView>> {
    ctx.require("client.card")?;
    check(&dto)?;
    let res = sqlx::query(
        "UPDATE clients SET name=$3, phone=COALESCE($4, phone), comment=COALESCE($5, comment), bonus_level=$6::float8 \
         WHERE id=$1 AND organization_id=$2 AND deleted_at IS NULL",
    )
    .bind(id)
    .bind(ctx.org_id)
    .bind(dto.name.trim())
    .bind(dto.phone.as_deref().map(normalize_phone))
    .bind(dto.comment)
    .bind(dto.bonus_level)
    .execute(&s.pool)
    .await
    .map_err(|e| conflict(e.into()))?;
    if res.rows_affected() == 0 {
        return Err(AppError::NotFound("Клиент не найден".into()));
    }
    one(State(s), ctx, Path(id)).await
}

async fn remove(State(s): State<AppState>, ctx: AuthContext, Path(id): Path<Uuid>) -> AppResult<Json<Value>> {
    ctx.require("client.delete")?;
    let res = sqlx::query("UPDATE clients SET deleted_at=now() WHERE id=$1 AND organization_id=$2 AND deleted_at IS NULL")
        .bind(id)
        .bind(ctx.org_id)
        .execute(&s.pool)
        .await?;
    if res.rows_affected() == 0 {
        return Err(AppError::NotFound("Клиент не найден".into()));
    }
    Ok(Json(json!({ "success": true })))
}

// ---------- отложенные чеки ----------
#[derive(Serialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct CartView {
    pub id: Uuid,
    pub store_id: Uuid,
    pub user_id: Uuid,
    pub name: String,
    pub payload: Value,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CartDto {
    pub store_id: Uuid,
    pub name: Option<String>,
    pub payload: Value,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CartQuery {
    pub store_id: Option<Uuid>,
}

async fn carts_list(State(s): State<AppState>, ctx: AuthContext, Query(q): Query<CartQuery>) -> AppResult<Json<Vec<CartView>>> {
    ctx.require("sale.new")?;
    let rows = sqlx::query_as::<_, CartView>(
        "SELECT id, store_id, user_id, name, payload FROM saved_carts \
         WHERE organization_id=$1 AND ($2::uuid IS NULL OR store_id=$2) ORDER BY created_at",
    )
    .bind(ctx.org_id)
    .bind(q.store_id)
    .fetch_all(&s.pool)
    .await?;
    Ok(Json(rows))
}

async fn carts_create(State(s): State<AppState>, ctx: AuthContext, Json(dto): Json<CartDto>) -> AppResult<Json<CartView>> {
    ctx.require("sale.new")?;
    if !dto.payload.is_object() {
        return Err(AppError::BadRequest("Корзина должна быть объектом".into()));
    }
    let (n,): (i64,) = sqlx::query_as("SELECT count(*) FROM stores WHERE id=$1 AND organization_id=$2 AND deleted_at IS NULL")
        .bind(dto.store_id)
        .bind(ctx.org_id)
        .fetch_one(&s.pool)
        .await?;
    if n == 0 {
        return Err(AppError::BadRequest("Филиал не найден".into()));
    }
    let id = Uuid::new_v4();
    let name = dto.name.unwrap_or_default();
    sqlx::query("INSERT INTO saved_carts (id, organization_id, store_id, user_id, name, payload) VALUES ($1,$2,$3,$4,$5,$6)")
        .bind(id)
        .bind(ctx.org_id)
        .bind(dto.store_id)
        .bind(ctx.user_id)
        .bind(&name)
        .bind(&dto.payload)
        .execute(&s.pool)
        .await?;
    Ok(Json(CartView { id, store_id: dto.store_id, user_id: ctx.user_id, name, payload: dto.payload }))
}

async fn carts_remove(State(s): State<AppState>, ctx: AuthContext, Path(id): Path<Uuid>) -> AppResult<Json<Value>> {
    ctx.require("sale.new")?;
    let res = sqlx::query("DELETE FROM saved_carts WHERE id=$1 AND organization_id=$2").bind(id).bind(ctx.org_id).execute(&s.pool).await?;
    if res.rows_affected() == 0 {
        return Err(AppError::NotFound("Чек не найден".into()));
    }
    Ok(Json(json!({ "success": true })))
}
