//! Вход с корпоративного сайта (`/api/auth/sso`) и закрытая регистрация (`ALLOW_SIGNUP=false`).
use atlas_api::{app, AppState, MIGRATOR};
use axum::{
    body::Body,
    http::{header, Method, Request, StatusCode},
    Router,
};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use sqlx::postgres::PgPoolOptions;
use tower::ServiceExt;

async fn call(router: &Router, method: Method, uri: &str, token: Option<&str>, sso: Option<&str>, body: Option<Value>) -> (StatusCode, Value) {
    let mut req = Request::builder().method(method).uri(uri);
    if let Some(t) = token {
        req = req.header(header::AUTHORIZATION, format!("Bearer {t}"));
    }
    if let Some(x) = sso {
        req = req.header("x-sso-secret", x);
    }
    let req = match body {
        Some(b) => req.header(header::CONTENT_TYPE, "application/json").body(Body::from(b.to_string())).unwrap(),
        None => req.body(Body::empty()).unwrap(),
    };
    let resp = router.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    (status, if bytes.is_empty() { Value::Null } else { serde_json::from_slice(&bytes).unwrap_or(Value::Null) })
}

#[tokio::test]
async fn sso_and_closed_signup() {
    let (url, _pg) = match std::env::var("TEST_DATABASE_URL") {
        Ok(u) => (u, None),
        Err(_) => {
            let mut pg = postgresql_embedded::PostgreSQL::default();
            pg.setup().await.expect("setup embedded postgres");
            pg.start().await.expect("start embedded postgres");
            pg.create_database("atlas_test").await.expect("create db");
            (pg.settings().url("atlas_test"), Some(pg))
        }
    };
    let pool = PgPoolOptions::new().max_connections(5).connect(&url).await.unwrap();
    sqlx::raw_sql("DROP SCHEMA public CASCADE; CREATE SCHEMA public;").execute(&pool).await.unwrap();
    MIGRATOR.run(&pool).await.unwrap();
    let router = app(AppState { pool, jwt_secret: "test-secret".into() });

    let (st, _) = call(&router, Method::POST, "/api/auth/signup", None, None, Some(json!({
        "email": "boss@example.com", "password": "secret1", "name": "Boss", "organizationName": "Shop"
    }))).await;
    assert_eq!(st, StatusCode::OK);

    // без SSO_SECRET вход с сайта выключен
    std::env::remove_var("SSO_SECRET");
    let (st, _) = call(&router, Method::POST, "/api/auth/sso", None, Some("x"), Some(json!({"email": "boss@example.com"}))).await;
    assert_eq!(st, StatusCode::NOT_FOUND);

    std::env::set_var("SSO_SECRET", "0123456789abcdef-secret");
    let (st, _) = call(&router, Method::POST, "/api/auth/sso", None, None, Some(json!({"email": "boss@example.com"}))).await;
    assert_eq!(st, StatusCode::UNAUTHORIZED);
    let (st, _) = call(&router, Method::POST, "/api/auth/sso", None, Some("wrong-secret-value-1"), Some(json!({"email": "boss@example.com"}))).await;
    assert_eq!(st, StatusCode::UNAUTHORIZED);
    let (st, _) = call(&router, Method::POST, "/api/auth/sso", None, Some("0123456789abcdef-secret"), Some(json!({"email": "nobody@example.com"}))).await;
    assert_eq!(st, StatusCode::NOT_FOUND);

    let (st, body) = call(&router, Method::POST, "/api/auth/sso", None, Some("0123456789abcdef-secret"), Some(json!({"email": " Boss@Example.com "}))).await;
    assert_eq!(st, StatusCode::OK, "{body}");
    let token = body["token"].as_str().unwrap().to_string();
    let (st, me) = call(&router, Method::GET, "/api/auth/me", Some(&token), None, None).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(me["email"], "boss@example.com");

    // закрытая регистрация
    let (_, cfg) = call(&router, Method::GET, "/api/auth/config", None, None, None).await;
    assert_eq!(cfg["signupEnabled"], true);
    std::env::set_var("ALLOW_SIGNUP", "false");
    let (_, cfg) = call(&router, Method::GET, "/api/auth/config", None, None, None).await;
    assert_eq!(cfg["signupEnabled"], false);
    let (st, _) = call(&router, Method::POST, "/api/auth/signup", None, None, Some(json!({
        "email": "x@example.com", "password": "secret1", "name": "X", "organizationName": "Y"
    }))).await;
    assert_eq!(st, StatusCode::FORBIDDEN);
}
