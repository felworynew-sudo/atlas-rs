//! Пароли (argon2id), JWT и экстрактор контекста авторизации.
use crate::{error::AppError, AppState};
use argon2::{
    password_hash::{phc::PasswordHash, PasswordHasher, PasswordVerifier},
    Argon2,
};
use axum::{extract::FromRequestParts, http::request::Parts};
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub fn hash_password(password: &str) -> Result<String, AppError> {
    Argon2::default()
        .hash_password(password.as_bytes())
        .map(|h| h.to_string())
        .map_err(|e| AppError::Internal(e.to_string()))
}

pub fn verify_password(password: &str, hash: &str) -> bool {
    match PasswordHash::new(hash) {
        Ok(parsed) => Argon2::default().verify_password(password.as_bytes(), &parsed).is_ok(),
        Err(_) => false,
    }
}

/// Правило Атласа для пароля: латиница/цифры/символы без пробелов, минимум 6.
pub fn validate_password(password: &str) -> Result<(), AppError> {
    if password.len() < 6 || password.chars().any(|c| c.is_whitespace() || !c.is_ascii()) {
        return Err(AppError::BadRequest(
            "Пароль: только латинские буквы без пробелов, минимум 6 символов".into(),
        ));
    }
    Ok(())
}

#[derive(Debug, Serialize, Deserialize)]
struct Claims {
    sub: Uuid,
    exp: usize,
}

pub fn issue_token(secret: &str, user_id: Uuid) -> Result<String, AppError> {
    issue_token_for(secret, user_id, chrono::Duration::days(30))
}

/// Токен с заданным сроком жизни (для входа с внешнего сайта срок короче).
pub fn issue_token_for(secret: &str, user_id: Uuid, ttl: chrono::Duration) -> Result<String, AppError> {
    let exp = (chrono::Utc::now() + ttl).timestamp() as usize;
    encode(&Header::default(), &Claims { sub: user_id, exp }, &EncodingKey::from_secret(secret.as_bytes()))
        .map_err(|e| AppError::Internal(e.to_string()))
}

/// Контекст запроса: пользователь, его организация и права (читаются из БД на каждый запрос).
#[derive(Debug, Clone)]
pub struct AuthContext {
    pub user_id: Uuid,
    pub org_id: Uuid,
    pub role_key: String,
    pub permissions: Vec<String>,
    /// Включённые модули организации (JSON `{"shifts": true, ...}`).
    pub modules: serde_json::Value,
}

impl AuthContext {
    pub fn is_admin(&self) -> bool {
        self.role_key == atlas_domain::roles::ADMIN_KEY
    }

    pub fn module_enabled(&self, module: atlas_domain::modules::Module) -> bool {
        self.modules.get(module.key()).and_then(serde_json::Value::as_bool).unwrap_or(false)
    }

    /// Право действует, если выдано роли (админ — все) и модуль, к которому оно относится, включён в организации.
    pub fn can(&self, permission: &str) -> bool {
        if let Some(m) = atlas_domain::permissions::module_of(permission) {
            if !self.module_enabled(m) {
                return false;
            }
        }
        self.is_admin() || self.permissions.iter().any(|p| p == permission)
    }

    pub fn require(&self, permission: &str) -> Result<(), AppError> {
        if self.can(permission) {
            Ok(())
        } else {
            Err(AppError::Forbidden)
        }
    }
}

impl FromRequestParts<AppState> for AuthContext {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Self::Rejection> {
        let header = parts
            .headers
            .get(axum::http::header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| AppError::Unauthorized("Требуется авторизация".into()))?;
        let token = header
            .strip_prefix("Bearer ")
            .ok_or_else(|| AppError::Unauthorized("Требуется авторизация".into()))?;
        let data = decode::<Claims>(
            token,
            &DecodingKey::from_secret(state.jwt_secret.as_bytes()),
            &Validation::default(),
        )
        .map_err(|_| AppError::Unauthorized("Сессия истекла".into()))?;

        let row: Option<(Uuid, String, Vec<String>, bool, serde_json::Value)> = sqlx::query_as(
            "SELECT u.organization_id, r.key, r.permissions, u.active, o.modules \
             FROM users u JOIN roles r ON r.id = u.role_id JOIN organizations o ON o.id = u.organization_id \
             WHERE u.id = $1",
        )
        .bind(data.claims.sub)
        .fetch_optional(&state.pool)
        .await?;

        match row {
            Some((org_id, role_key, permissions, true, modules)) => Ok(AuthContext {
                user_id: data.claims.sub,
                org_id,
                role_key,
                permissions,
                modules,
            }),
            _ => Err(AppError::Unauthorized("Сессия истекла".into())),
        }
    }
}
