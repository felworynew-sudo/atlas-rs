//! GET /api/organizations (текущая), PUT /api/organizations/{id}
use crate::{
    auth::AuthContext,
    error::{AppError, AppResult},
    AppState,
};
use atlas_domain::modules::ModuleFlags;
use axum::{
    extract::{Path, State},
    routing::get,
    Json, Router,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub fn router() -> Router<AppState> {
    Router::new().route("/", get(current)).route("/{id}", axum::routing::put(update))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OrgView {
    pub id: Uuid,
    pub name: String,
    pub currency: String,
    pub price_decimals: i32,
    pub modules: serde_json::Value,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateOrgDto {
    pub name: Option<String>,
    pub currency: Option<String>,
    pub price_decimals: Option<i32>,
    pub modules: Option<ModuleFlags>,
}

async fn fetch(state: &AppState, id: Uuid) -> AppResult<OrgView> {
    let row: Option<(Uuid, String, String, i32, serde_json::Value)> = sqlx::query_as(
        "SELECT id, name, currency, price_decimals, modules FROM organizations WHERE id = $1 AND deleted_at IS NULL",
    )
    .bind(id)
    .fetch_optional(&state.pool)
    .await?;
    let (id, name, currency, price_decimals, modules) =
        row.ok_or_else(|| AppError::NotFound("Организация не найдена".into()))?;
    Ok(OrgView { id, name, currency, price_decimals, modules })
}

async fn current(State(state): State<AppState>, ctx: AuthContext) -> AppResult<Json<OrgView>> {
    Ok(Json(fetch(&state, ctx.org_id).await?))
}

async fn update(
    State(state): State<AppState>,
    ctx: AuthContext,
    Path(id): Path<Uuid>,
    Json(dto): Json<UpdateOrgDto>,
) -> AppResult<Json<OrgView>> {
    if id != ctx.org_id {
        return Err(AppError::Forbidden);
    }
    ctx.require("settings.edit")?;
    if let Some(n) = &dto.name {
        if n.trim().is_empty() {
            return Err(AppError::BadRequest("Введите название".into()));
        }
    }
    if let Some(d) = dto.price_decimals {
        if !(0..=4).contains(&d) {
            return Err(AppError::BadRequest("Знаков после запятой: от 0 до 4".into()));
        }
    }
    let modules = match &dto.modules {
        Some(m) => Some(serde_json::to_value(m).map_err(|e| AppError::Internal(e.to_string()))?),
        None => None,
    };
    sqlx::query(
        "UPDATE organizations SET \
           name = COALESCE($2, name), currency = COALESCE($3, currency), \
           price_decimals = COALESCE($4, price_decimals), modules = COALESCE($5, modules) \
         WHERE id = $1",
    )
    .bind(id)
    .bind(dto.name.as_deref().map(str::trim))
    .bind(dto.currency)
    .bind(dto.price_decimals)
    .bind(modules)
    .execute(&state.pool)
    .await?;
    Ok(Json(fetch(&state, id).await?))
}
