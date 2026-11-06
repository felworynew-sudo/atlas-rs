//! /api/roles — роли и доступы; /api/permissions — справочник прав
use crate::{
    auth::AuthContext,
    error::{AppError, AppResult},
    AppState,
};
use atlas_domain::permissions;
use axum::{
    extract::{Path, State},
    routing::get,
    Json, Router,
};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

pub fn router() -> Router<AppState> {
    Router::new().route("/", get(list).post(create)).route("/{id}", get(one).put(update).delete(remove))
}

#[derive(Serialize)]
pub struct PermissionView {
    pub key: &'static str,
    pub title: &'static str,
    pub module: Option<atlas_domain::modules::Module>,
}

pub async fn list_permissions() -> Json<Vec<PermissionView>> {
    Json(permissions::ALL.iter().map(|p| PermissionView { key: p.key, title: p.title, module: p.module }).collect())
}

#[derive(Serialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct RoleView {
    pub id: Uuid,
    pub key: String,
    pub name: String,
    pub salary_type: String,
    pub salary_rate: f64,
    pub bonus_type: String,
    pub permissions: Vec<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RoleDto {
    pub name: String,
    pub salary_type: Option<String>,
    pub salary_rate: Option<f64>,
    pub bonus_type: Option<String>,
    pub permissions: Option<Vec<String>>,
}


fn validate(dto: &RoleDto) -> AppResult<()> {
    if dto.name.trim().is_empty() {
        return Err(AppError::BadRequest("Введите название".into()));
    }
    if let Some(t) = &dto.salary_type {
        if !["none", "hour", "day", "month"].contains(&t.as_str()) {
            return Err(AppError::BadRequest("Неизвестный тип зарплаты".into()));
        }
    }
    if let Some(t) = &dto.bonus_type {
        if !["none", "percent_own_sales"].contains(&t.as_str()) {
            return Err(AppError::BadRequest("Неизвестный тип бонусов".into()));
        }
    }
    if dto.salary_rate.map_or(false, |r| r < 0.0) {
        return Err(AppError::BadRequest("Ставка не может быть отрицательной".into()));
    }
    if let Some(ps) = &dto.permissions {
        if let Some(bad) = ps.iter().find(|p| !permissions::is_known(p)) {
            return Err(AppError::BadRequest(format!("Неизвестное право: {bad}")));
        }
    }
    Ok(())
}

async fn list(State(s): State<AppState>, ctx: AuthContext) -> AppResult<Json<Vec<RoleView>>> {
    ctx.require("settings.users")?;
    let rows = sqlx::query_as::<_, RoleView>("SELECT id, key, name, salary_type, salary_rate::float8 AS salary_rate, bonus_type, permissions FROM roles WHERE organization_id=$1 AND deleted_at IS NULL ORDER BY created_at")
    .bind(ctx.org_id)
    .fetch_all(&s.pool)
    .await?;
    Ok(Json(rows))
}

async fn one(State(s): State<AppState>, ctx: AuthContext, Path(id): Path<Uuid>) -> AppResult<Json<RoleView>> {
    ctx.require("settings.users")?;
    let row = sqlx::query_as::<_, RoleView>("SELECT id, key, name, salary_type, salary_rate::float8 AS salary_rate, bonus_type, permissions FROM roles WHERE id=$1 AND organization_id=$2 AND deleted_at IS NULL")
    .bind(id)
    .bind(ctx.org_id)
    .fetch_optional(&s.pool)
    .await?
    .ok_or_else(|| AppError::NotFound("Роль не найдена".into()))?;
    Ok(Json(row))
}

async fn create(State(s): State<AppState>, ctx: AuthContext, Json(dto): Json<RoleDto>) -> AppResult<Json<RoleView>> {
    ctx.require("settings.users")?;
    validate(&dto)?;
    let id = Uuid::new_v4();
    // Пользовательская роль получает ключ `custom_<uuid-префикс>`; ключи admin/cashier зарезервированы.
    let key = format!("custom_{}", &id.simple().to_string()[..8]);
    sqlx::query(
        "INSERT INTO roles (id, organization_id, key, name, salary_type, salary_rate, bonus_type, permissions) \
         VALUES ($1,$2,$3,$4,COALESCE($5,'none'),COALESCE($6::float8::numeric,0),COALESCE($7,'none'),COALESCE($8,'{}'))",
    )
    .bind(id)
    .bind(ctx.org_id)
    .bind(key)
    .bind(dto.name.trim())
    .bind(dto.salary_type)
    .bind(dto.salary_rate)
    .bind(dto.bonus_type)
    .bind(dto.permissions)
    .execute(&s.pool)
    .await?;
    one(State(s), ctx, Path(id)).await
}

async fn update(
    State(s): State<AppState>,
    ctx: AuthContext,
    Path(id): Path<Uuid>,
    Json(dto): Json<RoleDto>,
) -> AppResult<Json<RoleView>> {
    ctx.require("settings.users")?;
    validate(&dto)?;
    let current = one(State(s.clone()), ctx.clone(), Path(id)).await?.0;
    if current.key == atlas_domain::roles::ADMIN_KEY && dto.permissions.is_some() {
        return Err(AppError::BadRequest("Права роли «Админ» изменить нельзя".into()));
    }
    sqlx::query(
        "UPDATE roles SET name=$3, salary_type=COALESCE($4,salary_type), salary_rate=COALESCE($5::float8::numeric,salary_rate), \
         bonus_type=COALESCE($6,bonus_type), permissions=COALESCE($7,permissions) \
         WHERE id=$1 AND organization_id=$2 AND deleted_at IS NULL",
    )
    .bind(id)
    .bind(ctx.org_id)
    .bind(dto.name.trim())
    .bind(dto.salary_type)
    .bind(dto.salary_rate)
    .bind(dto.bonus_type)
    .bind(dto.permissions)
    .execute(&s.pool)
    .await?;
    one(State(s), ctx, Path(id)).await
}

async fn remove(State(s): State<AppState>, ctx: AuthContext, Path(id): Path<Uuid>) -> AppResult<Json<serde_json::Value>> {
    ctx.require("settings.users")?;
    let current = one(State(s.clone()), ctx.clone(), Path(id)).await?.0;
    if current.key == atlas_domain::roles::ADMIN_KEY || current.key == atlas_domain::roles::CASHIER_KEY {
        return Err(AppError::BadRequest("Системную роль удалить нельзя".into()));
    }
    let (used,): (i64,) = sqlx::query_as("SELECT count(*) FROM users WHERE role_id=$1 AND active")
        .bind(id)
        .fetch_one(&s.pool)
        .await?;
    if used > 0 {
        return Err(AppError::Conflict("Роль назначена сотрудникам".into()));
    }
    sqlx::query("UPDATE roles SET deleted_at = now() WHERE id=$1 AND organization_id=$2")
        .bind(id)
        .bind(ctx.org_id)
        .execute(&s.pool)
        .await?;
    Ok(Json(serde_json::json!({ "success": true })))
}
