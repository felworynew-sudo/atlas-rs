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

    // автозакрытие смен в время, заданное в настройках филиала (проверка раз в минуту)
    let job_pool = pool.clone();
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(std::time::Duration::from_secs(60));
        loop {
            tick.tick().await;
            match atlas_api::routes::shifts::auto_close_due(&job_pool, chrono::Utc::now()).await {
                Ok(n) if n > 0 => tracing::info!(closed = n, "автозакрытие смен"),
                Ok(_) => {}
                Err(e) => tracing::error!(error = %e, "ошибка автозакрытия смен"),
            }
        }
    });

    let listener = tokio::net::TcpListener::bind(&addr).await.expect("bind");
    tracing::info!(%addr, "atlas-api запущен");
    axum::serve(listener, app(AppState { pool, jwt_secret })).await.expect("serve");
}
