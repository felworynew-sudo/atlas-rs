//! Сквозной тест ядра: регистрация → вход → организации/филиалы/роли/сотрудники/права.
//! БД: переменная TEST_DATABASE_URL, иначе поднимается встроенный PostgreSQL (скачивается один раз).
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

struct Env {
    router: Router,
    _pg: Option<postgresql_embedded::PostgreSQL>,
}

async fn setup() -> Env {
    let (url, pg) = match std::env::var("TEST_DATABASE_URL") {
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
    MIGRATOR.run(&pool).await.unwrap();
    Env { router: app(AppState { pool, jwt_secret: "test-secret".into() }), _pg: pg }
}

async fn call(env: &Env, method: Method, uri: &str, token: Option<&str>, body: Option<Value>) -> (StatusCode, Value) {
    let mut req = Request::builder().method(method).uri(uri);
    if let Some(t) = token {
        req = req.header(header::AUTHORIZATION, format!("Bearer {t}"));
    }
    let req = if let Some(b) = body {
        req.header(header::CONTENT_TYPE, "application/json").body(Body::from(b.to_string())).unwrap()
    } else {
        req.body(Body::empty()).unwrap()
    };
    let resp = env.router.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let v = if bytes.is_empty() { Value::Null } else { serde_json::from_slice(&bytes).unwrap_or(Value::Null) };
    (status, v)
}

#[tokio::test]
async fn core_flow() {
    let env = setup().await;

    // health
    let (st, _) = call(&env, Method::GET, "/api/health", None, None).await;
    assert_eq!(st, StatusCode::OK);

    // без токена — 401
    let (st, body) = call(&env, Method::GET, "/api/stores", None, None).await;
    assert_eq!(st, StatusCode::UNAUTHORIZED);
    assert_eq!(body["statusCode"], 401);

    // регистрация: валидация пароля и успех
    let (st, body) = call(&env, Method::POST, "/api/auth/signup", None, Some(json!({
        "email": "owner@example.com", "password": "пароль", "name": "Owner", "organizationName": "Wow My Eyes"
    }))).await;
    assert_eq!(st, StatusCode::BAD_REQUEST, "{body}");

    let (st, body) = call(&env, Method::POST, "/api/auth/signup", None, Some(json!({
        "email": "owner@example.com", "password": "secret1", "name": "Owner",
        "organizationName": "Wow My Eyes", "storeName": "ТРЦ Галерея, 0 этаж"
    }))).await;
    assert_eq!(st, StatusCode::OK, "{body}");
    let token = body["token"].as_str().unwrap().to_string();
    assert_eq!(body["user"]["roleKey"], "admin");
    assert!(body["user"]["permissions"].as_array().unwrap().len() > 40);

    // повторный email
    let (st, _) = call(&env, Method::POST, "/api/auth/signup", None, Some(json!({
        "email": "OWNER@example.com", "password": "secret1", "name": "X", "organizationName": "Y"
    }))).await;
    assert_eq!(st, StatusCode::CONFLICT);

    // вход: неверный и верный пароль
    let (st, body) = call(&env, Method::POST, "/api/auth/login", None, Some(json!({"email":"owner@example.com","password":"bad"}))).await;
    assert_eq!(st, StatusCode::UNAUTHORIZED);
    assert_eq!(body["message"], "Неверный email или пароль");
    let (st, _) = call(&env, Method::POST, "/api/auth/login", None, Some(json!({"email":"owner@example.com","password":"secret1"}))).await;
    assert_eq!(st, StatusCode::OK);

    // организация: модули по умолчанию, обновление
    let (st, org) = call(&env, Method::GET, "/api/organizations", Some(&token), None).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(org["modules"]["simple_sales"], true);
    assert_eq!(org["modules"]["shifts"], false);
    let org_id = org["id"].as_str().unwrap().to_string();
    let (st, org) = call(&env, Method::PUT, &format!("/api/organizations/{org_id}"), Some(&token), Some(json!({
        "name": "Wow My Eyes 2", "modules": org["modules"].as_object().map(|m| {
            let mut m = m.clone(); m.insert("shifts".into(), json!(true)); m }).unwrap()
    }))).await;
    assert_eq!(st, StatusCode::OK, "{org}");
    assert_eq!(org["modules"]["shifts"], true);

    // филиалы
    let (st, stores) = call(&env, Method::GET, "/api/stores", Some(&token), None).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(stores.as_array().unwrap().len(), 1);
    let store1 = stores[0]["id"].as_str().unwrap().to_string();
    let (st, s2) = call(&env, Method::POST, "/api/stores", Some(&token), Some(json!({"name": "ТРЦ Галерея, 2 этаж", "autoCloseTime": "03:30"}))).await;
    assert_eq!(st, StatusCode::OK, "{s2}");
    let store2 = s2["id"].as_str().unwrap().to_string();
    let (st, _) = call(&env, Method::POST, "/api/stores", Some(&token), Some(json!({"name": "X", "autoCloseTime": "25:00"}))).await;
    assert_eq!(st, StatusCode::BAD_REQUEST);

    // роли: список, пользовательская роль, неизвестное право
    let (st, roles) = call(&env, Method::GET, "/api/roles", Some(&token), None).await;
    assert_eq!(st, StatusCode::OK);
    let cashier = roles.as_array().unwrap().iter().find(|r| r["key"] == "cashier").unwrap()["id"].as_str().unwrap().to_string();
    let (st, _) = call(&env, Method::POST, "/api/roles", Some(&token), Some(json!({"name":"Тест","permissions":["nope"]}))).await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
    let (st, custom) = call(&env, Method::POST, "/api/roles", Some(&token), Some(json!({"name":"Старший","salaryType":"hour","salaryRate":500.0,"permissions":["sale.new","sale.delete"]}))).await;
    assert_eq!(st, StatusCode::OK, "{custom}");

    // сотрудник-продавец
    let (st, u) = call(&env, Method::POST, "/api/users", Some(&token), Some(json!({
        "name": "Seller", "email": "seller@example.com", "password": "pass123", "roleId": cashier, "storeIds": [store1]
    }))).await;
    assert_eq!(st, StatusCode::OK, "{u}");
    let seller_id = u["id"].as_str().unwrap().to_string();

    let (_, login) = call(&env, Method::POST, "/api/auth/login", None, Some(json!({"email":"seller@example.com","password":"pass123"}))).await;
    let seller_token = login["token"].as_str().unwrap().to_string();

    // у продавца нет прав на настройки — 403; свой профиль доступен
    let (st, _) = call(&env, Method::POST, "/api/stores", Some(&seller_token), Some(json!({"name": "Z"}))).await;
    assert_eq!(st, StatusCode::FORBIDDEN);
    let (st, _) = call(&env, Method::GET, "/api/users", Some(&seller_token), None).await;
    assert_eq!(st, StatusCode::FORBIDDEN);
    let (st, me) = call(&env, Method::GET, "/api/auth/me", Some(&seller_token), None).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(me["roleKey"], "cashier");

    // нельзя деактивировать себя; деактивация продавца блокирует токен и вход
    let me_admin = call(&env, Method::GET, "/api/auth/me", Some(&token), None).await.1;
    let admin_id = me_admin["id"].as_str().unwrap().to_string();
    let (st, _) = call(&env, Method::PUT, &format!("/api/users/{admin_id}"), Some(&token), Some(json!({"active": false}))).await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
    let (st, _) = call(&env, Method::PUT, &format!("/api/users/{seller_id}"), Some(&token), Some(json!({"active": false, "storeIds": [store2]}))).await;
    assert_eq!(st, StatusCode::OK);
    let (st, _) = call(&env, Method::GET, "/api/auth/me", Some(&seller_token), None).await;
    assert_eq!(st, StatusCode::UNAUTHORIZED);
    let (st, inactive) = call(&env, Method::GET, "/api/users?inactive=true", Some(&token), None).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(inactive.as_array().unwrap().len(), 1);

    // системную роль удалить нельзя; пользовательскую — можно
    let (st, _) = call(&env, Method::DELETE, &format!("/api/roles/{cashier}"), Some(&token), None).await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
    let custom_id = custom["id"].as_str().unwrap();
    let (st, _) = call(&env, Method::DELETE, &format!("/api/roles/{custom_id}"), Some(&token), None).await;
    assert_eq!(st, StatusCode::OK);

    // удаление филиала: последний нельзя
    let (st, _) = call(&env, Method::DELETE, &format!("/api/stores/{store2}"), Some(&token), None).await;
    assert_eq!(st, StatusCode::OK);
    let (st, _) = call(&env, Method::DELETE, &format!("/api/stores/{store1}"), Some(&token), None).await;
    assert_eq!(st, StatusCode::BAD_REQUEST);

    // справочник прав
    let (st, perms) = call(&env, Method::GET, "/api/permissions", None, None).await;
    assert_eq!(st, StatusCode::OK);
    assert!(perms.as_array().unwrap().iter().any(|p| p["key"] == "sale.delete"));
}
