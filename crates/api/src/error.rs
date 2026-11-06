//! Единый тип ошибок; тело ответа совместимо с Атласом: {statusCode, message, error}.
use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("{0}")]
    BadRequest(String),
    #[error("{0}")]
    Unauthorized(String),
    #[error("Нет доступа")]
    Forbidden,
    #[error("{0}")]
    NotFound(String),
    #[error("{0}")]
    Conflict(String),
    #[error("Внутренняя ошибка сервера")]
    Internal(String),
}

impl AppError {
    pub fn status(&self) -> StatusCode {
        match self {
            AppError::BadRequest(_) => StatusCode::BAD_REQUEST,
            AppError::Unauthorized(_) => StatusCode::UNAUTHORIZED,
            AppError::Forbidden => StatusCode::FORBIDDEN,
            AppError::NotFound(_) => StatusCode::NOT_FOUND,
            AppError::Conflict(_) => StatusCode::CONFLICT,
            AppError::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    fn label(&self) -> &'static str {
        match self {
            AppError::BadRequest(_) => "Bad Request",
            AppError::Unauthorized(_) => "Unauthorized",
            AppError::Forbidden => "Forbidden",
            AppError::NotFound(_) => "Not Found",
            AppError::Conflict(_) => "Conflict",
            AppError::Internal(_) => "Internal Server Error",
        }
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        if let AppError::Internal(detail) = &self {
            tracing::error!(%detail, "internal error");
        }
        let status = self.status();
        let body = json!({
            "statusCode": status.as_u16(),
            "message": self.to_string(),
            "error": self.label(),
        });
        (status, Json(body)).into_response()
    }
}

impl From<sqlx::Error> for AppError {
    fn from(e: sqlx::Error) -> Self {
        if let sqlx::Error::Database(db) = &e {
            // 23505 = unique_violation
            if db.code().as_deref() == Some("23505") {
                return AppError::Conflict("Запись уже существует".into());
            }
        }
        AppError::Internal(e.to_string())
    }
}

pub type AppResult<T> = Result<T, AppError>;
