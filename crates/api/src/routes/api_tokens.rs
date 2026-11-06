//! Управление API-токенами (/api/api-tokens) и черновиками ИИ-агента на стороне пользователя (/api/api-drafts)
use crate::{
    auth::AuthContext,
    error::{AppError, AppResult},
    AppState,
};
use axum::{
    extract::{Path, Query, State},
    routing::{get, post},
    Json, Router,
};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::FromRow;
use uuid::Uuid;

/// Права, которые можно выдать токену сейчас.
pub const SCOPES: [&str; 6] = ["read", "read:products", "read:stock", "read:sales", "draft:sales", "draft:acceptances"];
/// Права, объявленные в интерфейсе как «скоро».
pub const COMING_SOON: [&str; 3] = ["write:sales", "write:products", "write:danger"];

pub fn tokens_router() -> Router<AppState> {
    Router::new().route("/", get(list).post(create)).route("/audit", get(audit)).route("/{id}/revoke", post(revoke))
}

pub fn drafts_router() -> Router<AppState> {
    Router::new().route("/", get(drafts_list)).route("/{id}", get(draft_one)).route("/{id}/applied", post(draft_applied)).route("/{id}/dismiss", post(draft_dismiss))
}

/// Секрет токена: `atl_` + 64 hex-символа; хранится только SHA-256.
pub fn new_secret() -> String {
    format!("atl_{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple())
}

pub async fn audit_log(pool: &sqlx::PgPool, org: Uuid, token: Option<Uuid>, user: Option<Uuid>, action: &str, allowed: bool, text: &str) {
    let res = sqlx::query("INSERT INTO api_audit_events (id, organization_id, token_id, user_id, action, allowed, text) VALUES ($1,$2,$3,$4,$5,$6,$7)")
        .bind(Uuid::new_v4())
        .bind(org)
        .bind(token)
        .bind(user)
        .bind(action)
        .bind(allowed)
        .bind(text)
        .execute(pool)
        .await;
    if let Err(e) = res {
        tracing::warn!(error = %e, "не удалось записать событие журнала API");
    }
}

#[derive(Serialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct TokenView {
    pub id: Uuid,
    pub name: String,
    pub prefix: String,
    pub scopes: Vec<String>,
    /// active | revoked | expired
    pub status: String,
    pub expires_at: DateTime<Utc>,
    pub last_used_at: Option<DateTime<Utc>>,
    pub created_by: String,
    pub created_at: DateTime<Utc>,
}

const SELECT_TOKENS: &str = "SELECT t.id, t.name, t.prefix, t.scopes, \
     CASE WHEN t.revoked_at IS NOT NULL THEN 'revoked' WHEN t.expires_at <= now() THEN 'expired' ELSE 'active' END AS status, \
     t.expires_at, t.last_used_at, u.name AS created_by, t.created_at \
     FROM api_tokens t JOIN users u ON u.id = t.created_by WHERE t.organization_id=$1 AND ($2::uuid IS NULL OR t.id=$2) ORDER BY t.created_at DESC";

async fn list(State(s): State<AppState>, ctx: AuthContext) -> AppResult<Json<Value>> {
    ctx.require("settings.edit")?;
    let tokens = sqlx::query_as::<_, TokenView>(SELECT_TOKENS).bind(ctx.org_id).bind(None::<Uuid>).fetch_all(&s.pool).await?;
    // «и может ли организация пользоваться API сейчас»: тарифов нет, API доступен всем
    Ok(Json(json!({ "canUseApi": true, "tokens": tokens })))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateDto {
    pub name: String,
    pub scopes: Vec<String>,
    pub expires_in_days: i64,
}

async fn create(State(s): State<AppState>, ctx: AuthContext, Json(dto): Json<CreateDto>) -> AppResult<Json<Value>> {
    ctx.require("settings.edit")?;
    if dto.name.trim().is_empty() {
        return Err(AppError::BadRequest("Введите название токена".into()));
    }
    if dto.scopes.is_empty() {
        return Err(AppError::BadRequest("Выберите хотя бы одно право".into()));
    }
    for sc in &dto.scopes {
        if COMING_SOON.contains(&sc.as_str()) {
            return Err(AppError::BadRequest(format!("Право «{sc}» появится позже (скоро)")));
        }
        if !SCOPES.contains(&sc.as_str()) {
            return Err(AppError::BadRequest(format!("Неизвестное право «{sc}»")));
        }
    }
    if ![30, 90, 365].contains(&dto.expires_in_days) {
        return Err(AppError::BadRequest("Срок действия: 30, 90 или 365 дней".into()));
    }
    let secret = new_secret();
    let id = Uuid::new_v4();
    let mut scopes = dto.scopes.clone();
    scopes.sort();
    scopes.dedup();
    sqlx::query(
        "INSERT INTO api_tokens (id, organization_id, name, prefix, token_hash, scopes, expires_at, created_by) \
         VALUES ($1,$2,$3,$4,encode(sha256(convert_to($5,'UTF8')),'hex'),$6,$7,$8)",
    )
    .bind(id)
    .bind(ctx.org_id)
    .bind(dto.name.trim())
    .bind(&secret[..12])
    .bind(&secret)
    .bind(&scopes)
    .bind(Utc::now() + Duration::days(dto.expires_in_days))
    .bind(ctx.user_id)
    .execute(&s.pool)
    .await?;
    audit_log(&s.pool, ctx.org_id, Some(id), Some(ctx.user_id), "token.created", true, &format!("Создан токен «{}»", dto.name.trim())).await;
    let view = sqlx::query_as::<_, TokenView>(SELECT_TOKENS).bind(ctx.org_id).bind(Some(id)).fetch_one(&s.pool).await?;
    // секрет возвращается один раз
    Ok(Json(json!({ "token": view, "secret": secret })))
}

async fn revoke(State(s): State<AppState>, ctx: AuthContext, Path(id): Path<Uuid>) -> AppResult<Json<TokenView>> {
    ctx.require("settings.edit")?;
    let res = sqlx::query("UPDATE api_tokens SET revoked_at = COALESCE(revoked_at, now()) WHERE id=$1 AND organization_id=$2")
        .bind(id)
        .bind(ctx.org_id)
        .execute(&s.pool)
        .await?;
    if res.rows_affected() == 0 {
        return Err(AppError::NotFound("Токен не найден".into()));
    }
    audit_log(&s.pool, ctx.org_id, Some(id), Some(ctx.user_id), "token.revoked", true, "Токен отозван").await;
    Ok(Json(sqlx::query_as::<_, TokenView>(SELECT_TOKENS).bind(ctx.org_id).bind(Some(id)).fetch_one(&s.pool).await?))
}

#[derive(Serialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct AuditView {
    pub id: Uuid,
    pub token_id: Option<Uuid>,
    pub token_name: Option<String>,
    pub action: String,
    pub allowed: bool,
    pub text: String,
    pub created_at: DateTime<Utc>,
}

async fn audit(State(s): State<AppState>, ctx: AuthContext) -> AppResult<Json<Vec<AuditView>>> {
    ctx.require("settings.edit")?;
    Ok(Json(
        sqlx::query_as::<_, AuditView>(
            "SELECT a.id, a.token_id, t.name AS token_name, a.action, a.allowed, a.text, a.created_at FROM api_audit_events a \
             LEFT JOIN api_tokens t ON t.id = a.token_id WHERE a.organization_id=$1 ORDER BY a.created_at DESC LIMIT 500",
        )
        .bind(ctx.org_id)
        .fetch_all(&s.pool)
        .await?,
    ))
}

// =========================== черновики (сторона пользователя) ===========================
#[derive(Serialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct DraftView {
    pub id: Uuid,
    pub kind: String,
    pub store_id: Uuid,
    pub payload: Value,
    /// proposed | expired | applied | dismissed
    pub status: String,
    pub expires_at: DateTime<Utc>,
    pub token_name: String,
    pub result: Option<Value>,
    pub created_at: DateTime<Utc>,
}

pub const SELECT_DRAFTS: &str = "SELECT d.id, d.kind, d.store_id, d.payload, \
     CASE WHEN d.status = 'proposed' AND d.expires_at <= now() THEN 'expired' ELSE d.status END AS status, \
     d.expires_at, t.name AS token_name, d.result, d.created_at \
     FROM api_drafts d JOIN api_tokens t ON t.id = d.token_id \
     WHERE d.organization_id=$1 AND ($2::uuid IS NULL OR d.id=$2) \
       AND ($3::text IS NULL OR (CASE WHEN d.status = 'proposed' AND d.expires_at <= now() THEN 'expired' ELSE d.status END) = $3) \
     ORDER BY d.created_at DESC LIMIT 500";

#[derive(Deserialize, Default)]
pub struct DraftQuery {
    /// по умолчанию — открытые; `all` — все
    pub status: Option<String>,
}

/// Пользователь видит черновики продаж с правом «Новая продажа» и черновики приёмок с правом «История приемок».
fn visible(ctx: &AuthContext, d: &DraftView) -> bool {
    match d.kind.as_str() {
        "sale" => ctx.can("sale.new"),
        _ => ctx.can("supply.history"),
    }
}

async fn drafts_list(State(s): State<AppState>, ctx: AuthContext, Query(q): Query<DraftQuery>) -> AppResult<Json<Vec<DraftView>>> {
    let status = match q.status.as_deref() {
        None => Some("proposed".to_string()),
        Some("all") => None,
        Some(other) => Some(other.to_string()),
    };
    let rows = sqlx::query_as::<_, DraftView>(SELECT_DRAFTS).bind(ctx.org_id).bind(None::<Uuid>).bind(status).fetch_all(&s.pool).await?;
    Ok(Json(rows.into_iter().filter(|d| visible(&ctx, d)).collect()))
}

async fn draft_get(s: &AppState, ctx: &AuthContext, id: Uuid) -> AppResult<DraftView> {
    let d = sqlx::query_as::<_, DraftView>(SELECT_DRAFTS)
        .bind(ctx.org_id)
        .bind(Some(id))
        .bind(None::<String>)
        .fetch_optional(&s.pool)
        .await?
        .ok_or_else(|| AppError::NotFound("Черновик не найден".into()))?;
    if !visible(ctx, &d) {
        return Err(AppError::Forbidden);
    }
    Ok(d)
}

async fn draft_one(State(s): State<AppState>, ctx: AuthContext, Path(id): Path<Uuid>) -> AppResult<Json<DraftView>> {
    Ok(Json(draft_get(&s, &ctx, id).await?))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppliedDto {
    pub sale_id: Option<Uuid>,
    pub supply_id: Option<Uuid>,
}

/// Приложение сообщает, что по черновику создана продажа/приёмка; сервер проверяет, что документ существует.
async fn draft_applied(State(s): State<AppState>, ctx: AuthContext, Path(id): Path<Uuid>, Json(dto): Json<AppliedDto>) -> AppResult<Json<DraftView>> {
    let d = draft_get(&s, &ctx, id).await?;
    if d.status != "proposed" {
        return Err(AppError::BadRequest("Черновик уже обработан или истёк".into()));
    }
    let result = if d.kind == "sale" {
        let sale = dto.sale_id.ok_or_else(|| AppError::BadRequest("Укажите созданную продажу".into()))?;
        let (n,): (i64,) = sqlx::query_as("SELECT count(*) FROM sales WHERE id=$1 AND organization_id=$2 AND store_id=$3 AND deleted_at IS NULL")
            .bind(sale)
            .bind(ctx.org_id)
            .bind(d.store_id)
            .fetch_one(&s.pool)
            .await?;
        if n == 0 {
            return Err(AppError::BadRequest("Продажа не найдена в этом филиале".into()));
        }
        json!({ "saleId": sale })
    } else {
        let supply = dto.supply_id.ok_or_else(|| AppError::BadRequest("Укажите созданную приёмку".into()))?;
        let (n,): (i64,) = sqlx::query_as("SELECT count(*) FROM supplies WHERE id=$1 AND organization_id=$2 AND store_id=$3 AND deleted_at IS NULL")
            .bind(supply)
            .bind(ctx.org_id)
            .bind(d.store_id)
            .fetch_one(&s.pool)
            .await?;
        if n == 0 {
            return Err(AppError::BadRequest("Приёмка не найдена в этом филиале".into()));
        }
        json!({ "supplyId": supply })
    };
    sqlx::query("UPDATE api_drafts SET status='applied', result=$2, resolved_at=now() WHERE id=$1 AND status='proposed'")
        .bind(id)
        .bind(&result)
        .execute(&s.pool)
        .await?;
    audit_log(&s.pool, ctx.org_id, None, Some(ctx.user_id), "draft.applied", true, &format!("Черновик «{}» проведён", d.token_name)).await;
    Ok(Json(draft_get(&s, &ctx, id).await?))
}

async fn draft_dismiss(State(s): State<AppState>, ctx: AuthContext, Path(id): Path<Uuid>) -> AppResult<Json<DraftView>> {
    let d = draft_get(&s, &ctx, id).await?;
    if d.status != "proposed" {
        return Err(AppError::BadRequest("Черновик уже обработан или истёк".into()));
    }
    sqlx::query("UPDATE api_drafts SET status='dismissed', resolved_at=now() WHERE id=$1 AND status='proposed'").bind(id).execute(&s.pool).await?;
    audit_log(&s.pool, ctx.org_id, None, Some(ctx.user_id), "draft.dismissed", true, &format!("Черновик «{}» отклонён", d.token_name)).await;
    Ok(Json(draft_get(&s, &ctx, id).await?))
}
