//! /api/stores — филиалы
use crate::{
    auth::AuthContext,
    error::{AppError, AppResult},
    AppState,
};
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

#[derive(Serialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct StoreView {
    pub id: Uuid,
    pub name: String,
    pub address: String,
    pub auto_close_time: String,
    pub timezone: String,
    pub show_stats_on_home: bool,
    pub fiscal_provider: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoreDto {
    pub name: String,
    pub address: Option<String>,
    pub auto_close_time: Option<String>,
    pub timezone: Option<String>,
    pub show_stats_on_home: Option<bool>,
    pub fiscal_provider: Option<String>,
}


fn validate(dto: &StoreDto) -> AppResult<()> {
    if dto.name.trim().is_empty() {
        return Err(AppError::BadRequest("Введите название".into()));
    }
    if let Some(t) = &dto.auto_close_time {
        let ok = t.len() == 5 && t.as_bytes()[2] == b':' && t[..2].parse::<u8>().map_or(false, |h| h < 24)
            && t[3..].parse::<u8>().map_or(false, |m| m < 60);
        if !ok {
            return Err(AppError::BadRequest("Время автозакрытия смены в формате ЧЧ:ММ".into()));
        }
    }
    if let Some(f) = &dto.fiscal_provider {
        if !["none", "light_kassa", "wipon_kassa"].contains(&f.as_str()) {
            return Err(AppError::BadRequest("Неизвестный оператор фискализации".into()));
        }
    }
    Ok(())
}

async fn list(State(s): State<AppState>, ctx: AuthContext) -> AppResult<Json<Vec<StoreView>>> {
    let rows = sqlx::query_as::<_, StoreView>("SELECT id, name, address, auto_close_time, timezone, show_stats_on_home, fiscal_provider FROM stores WHERE organization_id = $1 AND deleted_at IS NULL ORDER BY created_at")
    .bind(ctx.org_id)
    .fetch_all(&s.pool)
    .await?;
    Ok(Json(rows))
}

async fn one(State(s): State<AppState>, ctx: AuthContext, Path(id): Path<Uuid>) -> AppResult<Json<StoreView>> {
    let row = sqlx::query_as::<_, StoreView>("SELECT id, name, address, auto_close_time, timezone, show_stats_on_home, fiscal_provider FROM stores WHERE id = $1 AND organization_id = $2 AND deleted_at IS NULL")
    .bind(id)
    .bind(ctx.org_id)
    .fetch_optional(&s.pool)
    .await?
    .ok_or_else(|| AppError::NotFound("Филиал не найден".into()))?;
    Ok(Json(row))
}

async fn create(State(s): State<AppState>, ctx: AuthContext, Json(dto): Json<StoreDto>) -> AppResult<Json<StoreView>> {
    ctx.require("settings.edit")?;
    validate(&dto)?;
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO stores (id, organization_id, name, address, auto_close_time, timezone, show_stats_on_home, fiscal_provider) \
         VALUES ($1,$2,$3,$4,COALESCE($5,'00:00'),COALESCE($6,'Europe/Moscow'),COALESCE($7,true),COALESCE($8,'none'))",
    )
    .bind(id)
    .bind(ctx.org_id)
    .bind(dto.name.trim())
    .bind(dto.address.unwrap_or_default())
    .bind(dto.auto_close_time)
    .bind(dto.timezone)
    .bind(dto.show_stats_on_home)
    .bind(dto.fiscal_provider)
    .execute(&s.pool)
    .await?;
    one(State(s), ctx, Path(id)).await
}

async fn update(
    State(s): State<AppState>,
    ctx: AuthContext,
    Path(id): Path<Uuid>,
    Json(dto): Json<StoreDto>,
) -> AppResult<Json<StoreView>> {
    ctx.require("settings.edit")?;
    validate(&dto)?;
    let res = sqlx::query(
        "UPDATE stores SET name=$3, address=COALESCE($4,address), auto_close_time=COALESCE($5,auto_close_time), \
         timezone=COALESCE($6,timezone), show_stats_on_home=COALESCE($7,show_stats_on_home), \
         fiscal_provider=COALESCE($8,fiscal_provider) \
         WHERE id=$1 AND organization_id=$2 AND deleted_at IS NULL",
    )
    .bind(id)
    .bind(ctx.org_id)
    .bind(dto.name.trim())
    .bind(dto.address)
    .bind(dto.auto_close_time)
    .bind(dto.timezone)
    .bind(dto.show_stats_on_home)
    .bind(dto.fiscal_provider)
    .execute(&s.pool)
    .await?;
    if res.rows_affected() == 0 {
        return Err(AppError::NotFound("Филиал не найден".into()));
    }
    one(State(s), ctx, Path(id)).await
}

async fn remove(State(s): State<AppState>, ctx: AuthContext, Path(id): Path<Uuid>) -> AppResult<Json<serde_json::Value>> {
    ctx.require("settings.edit")?;
    let (count,): (i64,) =
        sqlx::query_as("SELECT count(*) FROM stores WHERE organization_id=$1 AND deleted_at IS NULL")
            .bind(ctx.org_id)
            .fetch_one(&s.pool)
            .await?;
    if count <= 1 {
        return Err(AppError::BadRequest("Нельзя удалить последний филиал".into()));
    }
    let res = sqlx::query("UPDATE stores SET deleted_at = now() WHERE id=$1 AND organization_id=$2 AND deleted_at IS NULL")
        .bind(id)
        .bind(ctx.org_id)
        .execute(&s.pool)
        .await?;
    if res.rows_affected() == 0 {
        return Err(AppError::NotFound("Филиал не найден".into()));
    }
    Ok(Json(serde_json::json!({ "success": true })))
}
