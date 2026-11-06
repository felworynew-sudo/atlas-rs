//! Atlas API: ядро (вход, организации, филиалы, роли, сотрудники).
pub mod auth;
pub mod error;
pub mod routes;

use axum::{routing::get, Router};
use sqlx::PgPool;

#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
    pub jwt_secret: String,
}

pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

pub fn app(state: AppState) -> Router {
    Router::new()
        .route("/api", get(routes::health::root))
        .route("/api/health", get(routes::health::detailed))
        .nest("/api/auth", routes::auth::router())
        .nest("/api/organizations", routes::organizations::router())
        .nest("/api/stores", routes::stores::router())
        .nest("/api/roles", routes::roles::router())
        .nest("/api/users", routes::users::router())
        .nest("/api/product-categories", routes::refs::categories_router())
        .nest("/api/suppliers", routes::refs::suppliers_router())
        .nest("/api/product-units", routes::refs::units_router())
        .nest("/api/products", routes::products::router())
        .route("/api/permissions", get(routes::roles::list_permissions))
        .with_state(state)
}
