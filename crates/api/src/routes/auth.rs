//! POST /api/auth/signup, POST /api/auth/login, GET /api/auth/me
use crate::{
    auth::{hash_password, issue_token, validate_password, verify_password, AuthContext},
    error::{AppError, AppResult},
    AppState,
};
use atlas_domain::{modules, roles};
use axum::{
    extract::State,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/config", get(config))
        .route("/sso", post(sso))
        .route("/signup", post(signup))
        .route("/login", post(login))
        .route("/me", get(me))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SignupDto {
    pub email: String,
    pub password: String,
    pub name: String,
    pub organization_name: String,
    pub store_name: Option<String>,
}

#[derive(Deserialize)]
pub struct LoginDto {
    pub email: String,
    pub password: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UserView {
    pub id: Uuid,
    pub organization_id: Uuid,
    pub email: String,
    pub name: String,
    pub role_id: Uuid,
    pub role_key: String,
    pub role_name: String,
    pub permissions: Vec<String>,
    pub store_ids: Vec<Uuid>,
}

#[derive(Serialize)]
pub struct LoginResponse {
    pub token: String,
    pub user: UserView,
}

fn check_email(email: &str) -> AppResult<String> {
    let e = email.trim().to_lowercase();
    if !e.contains('@') || e.len() < 5 || e.contains(' ') {
        return Err(AppError::BadRequest("Некорректный email".into()));
    }
    Ok(e)
}

pub async fn load_user_view(state: &AppState, user_id: Uuid) -> AppResult<UserView> {
    let row: Option<(Uuid, Uuid, String, String, Uuid, String, String, Vec<String>, Vec<Uuid>)> = sqlx::query_as(
        "SELECT u.id, u.organization_id, u.email, u.name, r.id, r.key, r.name, r.permissions, u.store_ids \
         FROM users u JOIN roles r ON r.id = u.role_id WHERE u.id = $1",
    )
    .bind(user_id)
    .fetch_optional(&state.pool)
    .await?;
    let (id, organization_id, email, name, role_id, role_key, role_name, perms, store_ids) =
        row.ok_or_else(|| AppError::NotFound("Пользователь не найден".into()))?;
    let permissions = if role_key == roles::ADMIN_KEY { atlas_domain::permissions::all_keys() } else { perms };
    Ok(UserView { id, organization_id, email, name, role_id, role_key, role_name, permissions, store_ids })
}

/// Регистрация новых компаний включена по умолчанию; для закрытой установки задайте `ALLOW_SIGNUP=false`.
fn signup_enabled() -> bool {
    !matches!(std::env::var("ALLOW_SIGNUP").ok().as_deref().map(str::trim), Some("false" | "0" | "no" | "off"))
}

async fn config() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "signupEnabled": signup_enabled() }))
}

async fn signup(State(state): State<AppState>, Json(dto): Json<SignupDto>) -> AppResult<Json<LoginResponse>> {
    if !signup_enabled() {
        return Err(AppError::Forbidden);
    }
    let email = check_email(&dto.email)?;
    validate_password(&dto.password)?;
    if dto.name.trim().is_empty() || dto.organization_name.trim().is_empty() {
        return Err(AppError::BadRequest("Введите название".into()));
    }
    let password_hash = hash_password(&dto.password)?;

    let mut tx = state.pool.begin().await?;
    let (org_id, store_id, admin_role_id) =
        create_organization(&mut tx, dto.organization_name.trim(), dto.store_name.as_deref().map(str::trim).filter(|s| !s.is_empty()).unwrap_or("Филиал 1")).await?;

    let user_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO users (id, organization_id, email, name, password_hash, role_id, store_ids) \
         VALUES ($1, $2, $3, $4, $5, $6, $7)",
    )
    .bind(user_id)
    .bind(org_id)
    .bind(&email)
    .bind(dto.name.trim())
    .bind(password_hash)
    .bind(admin_role_id)
    .bind(vec![store_id])
    .execute(&mut *tx)
    .await
    .map_err(|e| match AppError::from(e) {
        AppError::Conflict(_) => AppError::Conflict("Email уже используется".into()),
        other => other,
    })?;
    tx.commit().await?;

    let token = issue_token(&state.jwt_secret, user_id)?;
    Ok(Json(LoginResponse { token, user: load_user_view(&state, user_id).await? }))
}

/// Новая организация со всем, что нужно для работы: модули по умолчанию, первый филиал, роли, единицы, списки.
/// Возвращает (организация, филиал, роль «Админ»).
pub async fn create_organization(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    org_name: &str,
    store_name: &str,
) -> AppResult<(Uuid, Uuid, Uuid)> {
    let org_id = Uuid::new_v4();
    let flags = serde_json::to_value(modules::default_flags()).map_err(|e| AppError::Internal(e.to_string()))?;
    sqlx::query("INSERT INTO organizations (id, name, modules) VALUES ($1, $2, $3)")
        .bind(org_id)
        .bind(org_name)
        .bind(flags)
        .execute(&mut **tx)
        .await?;

    let store_id = Uuid::new_v4();
    sqlx::query("INSERT INTO stores (id, organization_id, name) VALUES ($1, $2, $3)")
        .bind(store_id)
        .bind(org_id)
        .bind(store_name)
        .execute(&mut **tx)
        .await?;

    let mut admin_role_id = Uuid::nil();
    for r in roles::defaults() {
        let id = Uuid::new_v4();
        if r.key == roles::ADMIN_KEY {
            admin_role_id = id;
        }
        sqlx::query("INSERT INTO roles (id, organization_id, key, name, permissions) VALUES ($1, $2, $3, $4, $5)")
            .bind(id)
            .bind(org_id)
            .bind(r.key)
            .bind(r.name)
            .bind(&r.permissions)
            .execute(&mut **tx)
            .await?;
    }

    super::refs::seed_default_units(tx, org_id).await?;
    super::lists::seed_defaults(tx, org_id).await?;
    Ok((org_id, store_id, admin_role_id))
}

async fn login(State(state): State<AppState>, Json(dto): Json<LoginDto>) -> AppResult<Json<LoginResponse>> {
    let email = dto.email.trim().to_lowercase();
    let row: Option<(Uuid, String, bool)> =
        sqlx::query_as("SELECT id, password_hash, active FROM users WHERE lower(email) = $1")
            .bind(&email)
            .fetch_optional(&state.pool)
            .await?;
    let bad = || AppError::Unauthorized("Неверный email или пароль".into());
    let (id, hash, active) = row.ok_or_else(bad)?;
    if !active || !verify_password(&dto.password, &hash) {
        return Err(bad());
    }
    let token = issue_token(&state.jwt_secret, id)?;
    Ok(Json(LoginResponse { token, user: load_user_view(&state, id).await? }))
}

async fn me(State(state): State<AppState>, ctx: AuthContext) -> AppResult<Json<UserView>> {
    Ok(Json(load_user_view(&state, ctx.user_id).await?))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SsoDto {
    #[serde(default)]
    pub email: Option<String>,
    /// id учётной записи на сайте (`users.external_id`, заводится через `POST /api/site/sync`).
    #[serde(default)]
    pub external_id: Option<String>,
}

/// Запрос от серверной части сайта: заголовок `X-Sso-Secret` совпадает с `SSO_SECRET` (не короче 16 символов).
pub fn check_site_secret(headers: &axum::http::HeaderMap) -> AppResult<()> {
    let expected = std::env::var("SSO_SECRET").ok().filter(|v| v.len() >= 16).ok_or(AppError::NotFound("Вход с сайта не настроен".into()))?;
    let given = headers.get("x-sso-secret").and_then(|v| v.to_str().ok()).unwrap_or("");
    if !same_secret(given, &expected) {
        return Err(AppError::Unauthorized("Неверный секрет входа с сайта".into()));
    }
    Ok(())
}

fn same_secret(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// Вход с корпоративного сайта: серверная часть сайта, уже проверившая свою сессию сотрудника, вызывает этот метод
/// с общим секретом (`SSO_SECRET`, заголовок `X-Sso-Secret`) и email сотрудника и получает токен Атласа на 12 часов.
/// Пользователь должен уже существовать в Атласе и быть активным; без `SSO_SECRET` метод выключен.
async fn sso(State(state): State<AppState>, headers: axum::http::HeaderMap, Json(dto): Json<SsoDto>) -> AppResult<Json<LoginResponse>> {
    check_site_secret(&headers)?;
    let row: Option<(Uuid, bool)> = match (dto.external_id.as_deref().map(str::trim).filter(|v| !v.is_empty()), dto.email.as_deref()) {
        (Some(ext), _) => sqlx::query_as(
            "SELECT u.id, u.active FROM users u JOIN organizations o ON o.id = u.organization_id              WHERE o.external_id IS NOT NULL AND u.external_id = $1",
        )
        .bind(ext)
        .fetch_optional(&state.pool)
        .await?,
        (None, Some(email)) => sqlx::query_as("SELECT id, active FROM users WHERE lower(email) = $1")
            .bind(email.trim().to_lowercase())
            .fetch_optional(&state.pool)
            .await?,
        (None, None) => return Err(AppError::BadRequest("Нужен email или externalId".into())),
    };
    let (id, active) = row.ok_or(AppError::NotFound("Сотрудник не заведён в Атласе".into()))?;
    if !active {
        return Err(AppError::Forbidden);
    }
    let token = crate::auth::issue_token_for(&state.jwt_secret, id, chrono::Duration::hours(12))?;
    Ok(Json(LoginResponse { token, user: load_user_view(&state, id).await? }))
}
