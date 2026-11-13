//! Atlas API: ядро (вход, организации, филиалы, роли, сотрудники).
pub mod auth;
pub mod error;
pub mod events;
pub mod ledger;
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
        .nest("/api/lists", routes::lists::router())
        .nest("/api/clients", routes::clients::router())
        .nest("/api/saved-carts", routes::clients::saved_carts_router())
        .nest("/api/sales", routes::sales::router())
        .nest("/api/stocks", routes::stocks::router())
        .nest("/api/supplies", routes::documents::supplies_router())
        .nest("/api/writeoffs", routes::documents::writeoffs_router())
        .nest("/api/revisions", routes::documents::revisions_router())
        .nest("/api/supply-orders", routes::supply_orders::router())
        .nest("/api/wallets", routes::finance::wallets_router())
        .nest("/api/spendings", routes::finance::spendings_router())
        .nest("/api/recurring-expenses", routes::finance::recurring_router())
        .nest("/api/reports", routes::reports::router())
        .nest("/api/complex-sales", routes::complex_sales::router())
        .nest("/api/complex-sale-templates", routes::complex_sales::templates_router())
        .nest("/api/orders", routes::orders::router())
        .nest("/api/marketplace", routes::orders::marketplace_router())
        .nest("/api/transfers", routes::transfers::transfers_router())
        .nest("/api/manufactures", routes::transfers::manufactures_router())
        .nest("/api/payments", routes::misc::payments_router())
        .nest("/api/updates", routes::misc::updates_router())
        .merge(routes::misc::labels_router())
        .nest("/api/price-types", routes::price_types::router())
        .nest("/api/api-tokens", routes::api_tokens::tokens_router())
        .nest("/api/api-drafts", routes::api_tokens::drafts_router())
        .nest("/api/v1", routes::v1::router())
        .nest("/api/shifts", routes::shifts::router())
        .nest("/api/site", routes::site::router())
        .nest("/api/salary", routes::salary::router())
        .nest("/api/salary-payments", routes::salary::payments_router())
        .nest("/api/working-hours", routes::salary::hours_router())
        .nest("/api/checklist", routes::checklist::router())
        .nest("/api/import", routes::import::import_router())
        .nest("/api/export", routes::import::export_router())
        .merge(routes::settings::router())
        .route("/api/permissions", get(routes::roles::list_permissions))
        .with_state(state)
}
