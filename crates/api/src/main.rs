use atlas_api::{app, AppState, MIGRATOR};
use sqlx::postgres::PgPoolOptions;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt().with_env_filter(
        std::env::var("RUST_LOG").unwrap_or_else(|_| "info".into()),
    ).init();

    let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL не задан");
    let jwt_secret = std::env::var("JWT_SECRET").expect("JWT_SECRET не задан");
    let addr = std::env::var("BIND").unwrap_or_else(|_| "0.0.0.0:8080".into());

    let pool = PgPoolOptions::new().max_connections(10).connect(&database_url).await.expect("подключение к БД");
    MIGRATOR.run(&pool).await.expect("миграции");

    let listener = tokio::net::TcpListener::bind(&addr).await.expect("bind");
    tracing::info!(%addr, "atlas-api запущен");
    axum::serve(listener, app(AppState { pool, jwt_secret })).await.expect("serve");
}
