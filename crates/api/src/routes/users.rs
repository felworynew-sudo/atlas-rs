//! /api/users — сотрудники
use crate::{
    auth::{hash_password, validate_password, AuthContext},
    error::{AppError, AppResult},
    routes::auth::{load_user_view, UserView},
    AppState,
};
use axum::{
    extract::{Path, Query, State},
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use uuid::Uuid;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list).post(create))
        .route("/{id}", get(one).put(update))
        .route("/{id}/password", post(set_password))
}

#[derive(Deserialize)]
pub struct ListQuery {
    pub inactive: Option<bool>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateUserDto {
    pub name: String,
    pub email: String,
    pub password: String,
    pub role_id: Uuid,
    pub store_ids: Vec<Uuid>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateUserDto {
    pub name: Option<String>,
    pub role_id: Option<Uuid>,
    pub store_ids: Option<Vec<Uuid>>,
    pub active: Option<bool>,
}

#[derive(Deserialize)]
pub struct PasswordDto {
    pub password: String,
}

async fn ensure_role_and_stores(s: &AppState, org: Uuid, role: Uuid, stores: &[Uuid]) -> AppResult<()> {
    let (r,): (i64,) = sqlx::query_as("SELECT count(*) FROM roles WHERE id=$1 AND organization_id=$2 AND deleted_at IS NULL")
        .bind(role)
        .bind(org)
        .fetch_one(&s.pool)
        .await?;
    if r == 0 {
        return Err(AppError::BadRequest("Роль не найдена".into()));
    }
    let (n,): (i64,) = sqlx::query_as(
        "SELECT count(*) FROM stores WHERE organization_id=$1 AND deleted_at IS NULL AND id = ANY($2)",
    )
    .bind(org)
    .bind(stores)
    .fetch_one(&s.pool)
    .await?;
    if n as usize != stores.len() {
        return Err(AppError::BadRequest("Филиал не найден".into()));
    }
    Ok(())
}

async fn list(State(s): State<AppState>, ctx: AuthContext, Query(q): Query<ListQuery>) -> AppResult<Json<Vec<UserView>>> {
    ctx.require("settings.users")?;
    let ids: Vec<(Uuid,)> = sqlx::query_as(
        "SELECT id FROM users WHERE organization_id=$1 AND active = $2 ORDER BY created_at",
    )
    .bind(ctx.org_id)
    .bind(!q.inactive.unwrap_or(false))
    .fetch_all(&s.pool)
    .await?;
    let mut out = Vec::with_capacity(ids.len());
    for (id,) in ids {
        out.push(load_user_view(&s, id).await?);
    }
    Ok(Json(out))
}

async fn one(State(s): State<AppState>, ctx: AuthContext, Path(id): Path<Uuid>) -> AppResult<Json<UserView>> {
    if id != ctx.user_id {
        ctx.require("settings.users")?;
    }
    let v = load_user_view(&s, id).await?;
    if v.organization_id != ctx.org_id {
        return Err(AppError::NotFound("Пользователь не найден".into()));
    }
    Ok(Json(v))
}

async fn create(State(s): State<AppState>, ctx: AuthContext, Json(dto): Json<CreateUserDto>) -> AppResult<Json<UserView>> {
    ctx.require("settings.users")?;
    if dto.name.trim().is_empty() {
        return Err(AppError::BadRequest("Введите имя".into()));
    }
    let email = dto.email.trim().to_lowercase();
    if !email.contains('@') {
        return Err(AppError::BadRequest("Некорректный email".into()));
    }
    validate_password(&dto.password)?;
    if dto.store_ids.is_empty() {
        return Err(AppError::BadRequest("Выберите филиал".into()));
    }
    ensure_role_and_stores(&s, ctx.org_id, dto.role_id, &dto.store_ids).await?;
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO users (id, organization_id, email, name, password_hash, role_id, store_ids) VALUES ($1,$2,$3,$4,$5,$6,$7)",
    )
    .bind(id)
    .bind(ctx.org_id)
    .bind(&email)
    .bind(dto.name.trim())
    .bind(hash_password(&dto.password)?)
    .bind(dto.role_id)
    .bind(&dto.store_ids)
    .execute(&s.pool)
    .await
    .map_err(|e| match AppError::from(e) {
        AppError::Conflict(_) => AppError::Conflict("Email уже используется".into()),
        o => o,
    })?;
    Ok(Json(load_user_view(&s, id).await?))
}

async fn update(
    State(s): State<AppState>,
    ctx: AuthContext,
    Path(id): Path<Uuid>,
    Json(dto): Json<UpdateUserDto>,
) -> AppResult<Json<UserView>> {
    ctx.require("settings.users")?;
    let current = load_user_view(&s, id).await?;
    if current.organization_id != ctx.org_id {
        return Err(AppError::NotFound("Пользователь не найден".into()));
    }
    if id == ctx.user_id && dto.active == Some(false) {
        return Err(AppError::BadRequest("Нельзя деактивировать текущего пользователя".into()));
    }
    let role = dto.role_id.unwrap_or(current.role_id);
    let stores = dto.store_ids.clone().unwrap_or_else(|| current.store_ids.clone());
    ensure_role_and_stores(&s, ctx.org_id, role, &stores).await?;
    sqlx::query(
        "UPDATE users SET name=COALESCE($2,name), role_id=$3, store_ids=$4, active=COALESCE($5,active) WHERE id=$1",
    )
    .bind(id)
    .bind(dto.name.as_deref().map(str::trim))
    .bind(role)
    .bind(&stores)
    .bind(dto.active)
    .execute(&s.pool)
    .await?;
    Ok(Json(load_user_view(&s, id).await?))
}

async fn set_password(
    State(s): State<AppState>,
    ctx: AuthContext,
    Path(id): Path<Uuid>,
    Json(dto): Json<PasswordDto>,
) -> AppResult<Json<serde_json::Value>> {
    if id != ctx.user_id {
        ctx.require("settings.users")?;
    }
    validate_password(&dto.password)?;
    let res = sqlx::query("UPDATE users SET password_hash=$2 WHERE id=$1 AND organization_id=$3")
        .bind(id)
        .bind(hash_password(&dto.password)?)
        .bind(ctx.org_id)
        .execute(&s.pool)
        .await?;
    if res.rows_affected() == 0 {
        return Err(AppError::NotFound("Пользователь не найден".into()));
    }
    Ok(Json(serde_json::json!({ "success": true })))
}
