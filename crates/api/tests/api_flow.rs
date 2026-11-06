//! Сквозной тест шага 9: API-токены организации, Atlas API v1, журнал действий и черновики ИИ-агента.
use atlas_api::{app, AppState, MIGRATOR};
use axum::{
    body::Body,
    http::{header, Method, Request, StatusCode},
    Router,
};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use sqlx::{postgres::PgPoolOptions, PgPool};
use tower::ServiceExt;

struct Env {
    router: Router,
    pool: PgPool,
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
    sqlx::raw_sql("DROP SCHEMA public CASCADE; CREATE SCHEMA public;").execute(&pool).await.unwrap();
    MIGRATOR.run(&pool).await.unwrap();
    Env { router: app(AppState { pool: pool.clone(), jwt_secret: "test-secret".into() }), pool, _pg: pg }
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
    (status, if bytes.is_empty() { Value::Null } else { serde_json::from_slice(&bytes).unwrap_or(Value::Null) })
}

fn s(v: &Value) -> String {
    v.as_str().unwrap_or_else(|| panic!("not a string: {v}")).to_string()
}

fn eq(v: &Value, x: f64) {
    let got = v.as_f64().unwrap_or_else(|| panic!("not a number: {v}"));
    assert!((got - x).abs() < 0.005, "expected {x}, got {got}");
}

#[tokio::test]
async fn api_flow() {
    let env = setup().await;
    let (st, body) = call(&env, Method::POST, "/api/auth/signup", None, Some(json!({
        "email": "api@example.com", "password": "secret1", "name": "Owner", "organizationName": "Shop", "storeName": "Точка"
    }))).await;
    assert_eq!(st, StatusCode::OK, "{body}");
    let t = s(&body["token"]);
    let t = t.as_str();
    let (_, org) = call(&env, Method::GET, "/api/organizations", Some(t), None).await;
    let mut modules = org["modules"].clone();
    modules["supplies"] = json!(true);
    call(&env, Method::PUT, &format!("/api/organizations/{}", s(&org["id"])), Some(t), Some(json!({"modules": modules}))).await;
    let (_, stores) = call(&env, Method::GET, "/api/stores", Some(t), None).await;
    let store = s(&stores[0]["id"]);
    let (_, milk) = call(&env, Method::POST, "/api/products", Some(t), Some(json!({
        "name": "Молоко", "purchasePrice": 60.0, "stores": [{"storeId": store, "salePrice": 100.0}]
    }))).await;
    let milk = s(&milk["id"]);
    call(&env, Method::PUT, &format!("/api/stocks/{milk}"), Some(t), Some(json!({"storeId": store, "qty": 10}))).await;

    // ---- выпуск токенов ----
    let (_, none) = call(&env, Method::GET, "/api/api-tokens", Some(t), None).await;
    assert_eq!(none["canUseApi"], true);
    assert_eq!(none["tokens"].as_array().unwrap().len(), 0);
    for body in [
        json!({"name": " ", "scopes": ["read"], "expiresInDays": 30}),
        json!({"name": "X", "scopes": [], "expiresInDays": 30}),
        json!({"name": "X", "scopes": ["root"], "expiresInDays": 30}),
        json!({"name": "X", "scopes": ["write:sales"], "expiresInDays": 30}),
        json!({"name": "X", "scopes": ["read"], "expiresInDays": 7}),
    ] {
        let (st, e) = call(&env, Method::POST, "/api/api-tokens", Some(t), Some(body.clone())).await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "{body} -> {e}");
    }
    let (st, soon) = call(&env, Method::POST, "/api/api-tokens", Some(t), Some(json!({"name": "X", "scopes": ["write:danger"], "expiresInDays": 30}))).await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
    assert!(s(&soon["message"]).contains("скоро"));

    let (st, ta) = call(&env, Method::POST, "/api/api-tokens", Some(t), Some(json!({"name": "claude", "scopes": ["read:products", "draft:sales"], "expiresInDays": 30}))).await;
    assert_eq!(st, StatusCode::OK, "{ta}");
    let secret_a = s(&ta["secret"]);
    assert!(secret_a.starts_with("atl_") && secret_a.len() == 68, "{secret_a}");
    assert_eq!(ta["token"]["prefix"], secret_a[..12]);
    assert_eq!(ta["token"]["status"], "active");
    assert!(ta["token"].get("secret").is_none());
    let id_a = s(&ta["token"]["id"]);
    let (_, tb) = call(&env, Method::POST, "/api/api-tokens", Some(t), Some(json!({"name": "reports", "scopes": ["read"], "expiresInDays": 90}))).await;
    let (secret_b, id_b) = (s(&tb["secret"]), s(&tb["token"]["id"]));
    let (_, tc) = call(&env, Method::POST, "/api/api-tokens", Some(t), Some(json!({"name": "buyer", "scopes": ["draft:acceptances"], "expiresInDays": 365}))).await;
    let secret_c = s(&tc["secret"]);
    // список не содержит секретов
    let (_, listed) = call(&env, Method::GET, "/api/api-tokens", Some(t), None).await;
    assert_eq!(listed["tokens"].as_array().unwrap().len(), 3);
    assert!(!listed.to_string().contains(&secret_a));

    // ---- аутентификация v1 ----
    let (st, _) = call(&env, Method::GET, "/api/v1/whoami", None, None).await;
    assert_eq!(st, StatusCode::UNAUTHORIZED);
    let (st, _) = call(&env, Method::GET, "/api/v1/whoami", Some("atl_deadbeef"), None).await;
    assert_eq!(st, StatusCode::UNAUTHORIZED);
    let (st, _) = call(&env, Method::GET, "/api/v1/whoami", Some(t), None).await; // сессионный токен не подходит
    assert_eq!(st, StatusCode::UNAUTHORIZED);
    let (st, who) = call(&env, Method::GET, "/api/v1/whoami", Some(&secret_a), None).await;
    assert_eq!(st, StatusCode::OK, "{who}");
    assert_eq!(who["organization"]["name"], "Shop");
    assert_eq!(who["token"]["name"], "claude");

    // ---- права токена ----
    let (st, prods) = call(&env, Method::GET, "/api/v1/products", Some(&secret_a), None).await;
    assert_eq!(st, StatusCode::OK, "{prods}");
    assert_eq!(prods[0]["name"], "Молоко");
    let (st, one) = call(&env, Method::GET, &format!("/api/v1/products/{milk}"), Some(&secret_a), None).await;
    assert_eq!(st, StatusCode::OK);
    eq(&one["stores"][0]["salePrice"], 100.0);
    for uri in ["/api/v1/sales", "/api/v1/reports/stock-balances", "/api/v1/acceptances", "/api/v1/reports/sales-summary"] {
        let (st, _) = call(&env, Method::GET, uri, Some(&secret_a), None).await;
        assert_eq!(st, StatusCode::FORBIDDEN, "{uri}");
    }
    let (st, _) = call(&env, Method::POST, "/api/v1/acceptances/drafts", Some(&secret_a), Some(json!({"storeId": store, "items": [{"productId": milk, "qty": 1, "price": 1.0}]}))).await;
    assert_eq!(st, StatusCode::FORBIDDEN);

    // данные для отчётов
    let (_, sale) = call(&env, Method::POST, "/api/sales", Some(t), Some(json!({"storeId": store, "items": [{"productId": milk, "qty": 2}]}))).await;
    let sale_id = s(&sale["id"]);

    // «чтение — всё»
    let (st, sl) = call(&env, Method::GET, "/api/v1/sales", Some(&secret_b), None).await;
    assert_eq!(st, StatusCode::OK, "{sl}");
    assert_eq!(sl.as_array().unwrap().len(), 1);
    let (st, so) = call(&env, Method::GET, &format!("/api/v1/sales/{sale_id}"), Some(&secret_b), None).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(so["items"].as_array().unwrap().len(), 1);
    let (_, rev) = call(&env, Method::GET, "/api/v1/reports/revenue-by-day", Some(&secret_b), None).await;
    assert_eq!(rev.as_array().unwrap().len(), 1);
    eq(&rev[0]["revenue"], 200.0);
    let (_, sum) = call(&env, Method::GET, "/api/v1/reports/sales-summary", Some(&secret_b), None).await;
    eq(&sum["revenue"], 200.0);
    assert_eq!(sum["salesCount"], 1);
    eq(&sum["avgCheck"], 200.0);
    assert_eq!(sum["bestSellers"][0]["name"], "Молоко");
    let (_, top) = call(&env, Method::GET, "/api/v1/reports/top-products", Some(&secret_b), None).await;
    eq(&top[0]["qty"], 2.0);
    let (_, bal) = call(&env, Method::GET, "/api/v1/reports/stock-balances", Some(&secret_b), None).await;
    eq(&bal[0]["stores"][0]["qty"], 8.0);
    let (st, one_bal) = call(&env, Method::GET, &format!("/api/v1/reports/stock-balances/{milk}"), Some(&secret_b), None).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(one_bal["name"], "Молоко");
    let (st, _) = call(&env, Method::GET, "/api/v1/acceptances", Some(&secret_b), None).await;
    assert_eq!(st, StatusCode::OK);
    let (st, _) = call(&env, Method::POST, "/api/v1/sales/drafts", Some(&secret_b), Some(json!({"storeId": store, "items": [{"productId": milk, "qty": 1}]}))).await;
    assert_eq!(st, StatusCode::FORBIDDEN); // «чтение» не даёт создавать черновики

    // ---- черновики агента ----
    for bad in [
        json!({"storeId": store, "items": []}),
        json!({"storeId": store, "items": [{"productId": "00000000-0000-0000-0000-000000000001", "qty": 1}]}),
        json!({"storeId": store, "items": [{"productId": milk, "qty": 0}]}),
        json!({"storeId": "00000000-0000-0000-0000-000000000002", "items": [{"productId": milk, "qty": 1}]}),
    ] {
        let (st, e) = call(&env, Method::POST, "/api/v1/sales/drafts", Some(&secret_a), Some(bad.clone())).await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "{bad} -> {e}");
    }
    let (st, sd) = call(&env, Method::POST, "/api/v1/sales/drafts", Some(&secret_a), Some(json!({
        "storeId": store, "items": [{"productId": milk, "qty": 3}], "clientPhone": "+79001234567", "comment": "Заказ из чата"
    }))).await;
    assert_eq!(st, StatusCode::OK, "{sd}");
    assert_eq!(sd["status"], "proposed");
    assert_eq!(sd["kind"], "sale");
    let sd_id = s(&sd["id"]);
    // ничего не продано и остаток не изменился
    let (_, rows) = call(&env, Method::GET, &format!("/api/stocks?storeId={store}"), Some(t), None).await;
    eq(&rows.as_array().unwrap().iter().find(|r| r["productId"] == milk.as_str()).unwrap()["qty"], 8.0);
    let (st, _) = call(&env, Method::POST, "/api/v1/acceptances/drafts", Some(&secret_c), Some(json!({"storeId": store, "items": [{"productId": milk, "qty": 5}]}))).await;
    assert_eq!(st, StatusCode::BAD_REQUEST); // в приёмке нужна цена
    let (st, ad) = call(&env, Method::POST, "/api/v1/acceptances/drafts", Some(&secret_c), Some(json!({"storeId": store, "items": [{"productId": milk, "qty": 5, "price": 55.0}]}))).await;
    assert_eq!(st, StatusCode::OK, "{ad}");
    let ad_id = s(&ad["id"]);
    let (_, all) = call(&env, Method::GET, "/api/v1/drafts", Some(&secret_b), None).await;
    assert_eq!(all.as_array().unwrap().len(), 2);
    let (st, d1) = call(&env, Method::GET, &format!("/api/v1/drafts/{sd_id}"), Some(&secret_a), None).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(d1["tokenName"], "claude");

    // сторона пользователя: открытые черновики, проведение и отклонение
    let (_, open) = call(&env, Method::GET, "/api/api-drafts", Some(t), None).await;
    assert_eq!(open.as_array().unwrap().len(), 2);
    let (st, _) = call(&env, Method::POST, &format!("/api/api-drafts/{sd_id}/applied"), Some(t), Some(json!({}))).await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
    let (st, _) = call(&env, Method::POST, &format!("/api/api-drafts/{sd_id}/applied"), Some(t), Some(json!({"saleId": "00000000-0000-0000-0000-000000000003"}))).await;
    assert_eq!(st, StatusCode::BAD_REQUEST); // сервер проверяет, что продажа существует
    let (st, applied) = call(&env, Method::POST, &format!("/api/api-drafts/{sd_id}/applied"), Some(t), Some(json!({"saleId": sale_id}))).await;
    assert_eq!(st, StatusCode::OK, "{applied}");
    assert_eq!(applied["status"], "applied");
    assert_eq!(applied["result"]["saleId"], sale_id.as_str());
    let (st, _) = call(&env, Method::POST, &format!("/api/api-drafts/{sd_id}/dismiss"), Some(t), None).await;
    assert_eq!(st, StatusCode::BAD_REQUEST); // уже обработан
    let (st, dis) = call(&env, Method::POST, &format!("/api/api-drafts/{ad_id}/dismiss"), Some(t), None).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(dis["status"], "dismissed");
    let (_, open) = call(&env, Method::GET, "/api/api-drafts", Some(t), None).await;
    assert_eq!(open.as_array().unwrap().len(), 0);
    let (_, everything) = call(&env, Method::GET, "/api/api-drafts?status=all", Some(t), None).await;
    assert_eq!(everything.as_array().unwrap().len(), 2);
    let (_, seen) = call(&env, Method::GET, &format!("/api/v1/drafts/{ad_id}"), Some(&secret_c), None).await;
    assert_eq!(seen["status"], "dismissed"); // агент видит, что стало с черновиком

    // срок черновика истёк
    let (_, sd2) = call(&env, Method::POST, "/api/v1/sales/drafts", Some(&secret_a), Some(json!({"storeId": store, "items": [{"productId": milk, "qty": 1}]}))).await;
    let sd2_id = s(&sd2["id"]);
    sqlx::query("UPDATE api_drafts SET expires_at = now() - interval '1 hour' WHERE id = $1::uuid").bind(&sd2_id).execute(&env.pool).await.unwrap();
    let (_, exp) = call(&env, Method::GET, &format!("/api/v1/drafts/{sd2_id}"), Some(&secret_a), None).await;
    assert_eq!(exp["status"], "expired");
    let (st, _) = call(&env, Method::POST, &format!("/api/api-drafts/{sd2_id}/applied"), Some(t), Some(json!({"saleId": sale_id}))).await;
    assert_eq!(st, StatusCode::BAD_REQUEST);

    // ---- журнал действий ----
    let (st, audit) = call(&env, Method::GET, "/api/api-tokens/audit", Some(t), None).await;
    assert_eq!(st, StatusCode::OK);
    let texts: Vec<String> = audit.as_array().unwrap().iter().map(|e| s(&e["text"])).collect();
    assert!(texts.contains(&"Агент «claude» запросил товары".to_string()), "{texts:?}");
    assert!(texts.contains(&"Агент «claude» запросил продажи — отказано".to_string()), "{texts:?}");
    assert!(texts.contains(&"Агент «buyer» предложил приёмку".to_string()), "{texts:?}");
    let denied = audit.as_array().unwrap().iter().find(|e| s(&e["text"]).ends_with("— отказано")).unwrap();
    assert_eq!(denied["allowed"], false);

    // ---- отзыв и срок действия ----
    let (_, listed) = call(&env, Method::GET, "/api/api-tokens", Some(t), None).await;
    assert!(listed["tokens"].as_array().unwrap().iter().all(|x| !x["lastUsedAt"].is_null())); // каждый токен использовался
    let (st, rv) = call(&env, Method::POST, &format!("/api/api-tokens/{id_a}/revoke"), Some(t), None).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(rv["status"], "revoked");
    let (st, _) = call(&env, Method::POST, &format!("/api/api-tokens/{id_a}/revoke"), Some(t), None).await;
    assert_eq!(st, StatusCode::OK); // идемпотентно
    let (st, _) = call(&env, Method::GET, "/api/v1/whoami", Some(&secret_a), None).await;
    assert_eq!(st, StatusCode::UNAUTHORIZED);
    sqlx::query("UPDATE api_tokens SET expires_at = now() - interval '1 minute' WHERE id = $1::uuid").bind(&id_b).execute(&env.pool).await.unwrap();
    let (st, _) = call(&env, Method::GET, "/api/v1/whoami", Some(&secret_b), None).await;
    assert_eq!(st, StatusCode::UNAUTHORIZED);
    let (_, listed) = call(&env, Method::GET, "/api/api-tokens", Some(t), None).await;
    let status_of = |id: &str| s(&listed["tokens"].as_array().unwrap().iter().find(|x| x["id"] == id).unwrap()["status"]);
    assert_eq!(status_of(&id_b), "expired");

    // токены создаёт только тот, кто может менять настройки
    let (_, roles) = call(&env, Method::GET, "/api/roles", Some(t), None).await;
    let cashier = s(&roles.as_array().unwrap().iter().find(|r| r["key"] == "cashier").unwrap()["id"]);
    call(&env, Method::POST, "/api/users", Some(t), Some(json!({
        "name": "S", "email": "seller-api@example.com", "password": "pass123", "roleId": cashier, "storeIds": [store]
    }))).await;
    let (_, login) = call(&env, Method::POST, "/api/auth/login", None, Some(json!({"email": "seller-api@example.com", "password": "pass123"}))).await;
    let ct = s(&login["token"]);
    let (st, _) = call(&env, Method::GET, "/api/api-tokens", Some(&ct), None).await;
    assert_eq!(st, StatusCode::FORBIDDEN);
    let (st, _) = call(&env, Method::POST, "/api/api-tokens", Some(&ct), Some(json!({"name": "x", "scopes": ["read"], "expiresInDays": 30}))).await;
    assert_eq!(st, StatusCode::FORBIDDEN);
}
