//! Зачисление выручки на счета методов оплаты: продажа, смешанная оплата, возврат, удаление.
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

async fn call(router: &Router, method: Method, uri: &str, token: Option<&str>, body: Option<Value>) -> (StatusCode, Value) {
    let mut req = Request::builder().method(method).uri(uri);
    if let Some(t) = token {
        req = req.header(header::AUTHORIZATION, format!("Bearer {t}"));
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

fn balance(wallets: &Value, id: &str) -> f64 {
    wallets.as_array().unwrap().iter().find(|w| w["id"] == id).unwrap()["balance"].as_f64().unwrap()
}

#[tokio::test]
async fn sales_settle_to_wallets() {
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

    let (_, body) = call(&router, Method::POST, "/api/auth/signup", None, Some(json!({
        "email": "s@example.com", "password": "secret1", "name": "S", "organizationName": "Shop", "storeName": "Точка"
    }))).await;
    let t = body["token"].as_str().unwrap().to_string();
    let t = Some(t.as_str());

    let (_, stores) = call(&router, Method::GET, "/api/stores", t, None).await;
    let store = stores[0]["id"].as_str().unwrap().to_string();
    let (_, cash_w) = call(&router, Method::POST, "/api/wallets", t, Some(json!({"name": "Касса", "storeId": store}))).await;
    let (_, bank_w) = call(&router, Method::POST, "/api/wallets", t, Some(json!({"name": "Банк"}))).await;
    let (cash_w, bank_w) = (cash_w["id"].as_str().unwrap().to_string(), bank_w["id"].as_str().unwrap().to_string());

    let (_, methods) = call(&router, Method::GET, "/api/lists/payment_methods", t, None).await;
    let cash = methods.as_array().unwrap().iter().find(|m| m["params"]["kind"] == "cash").unwrap().clone();
    let qr = methods.as_array().unwrap().iter().find(|m| m["params"]["kind"] == "qr").unwrap().clone();
    for (m, w) in [(&cash, &cash_w), (&qr, &bank_w)] {
        let (st, r) = call(&router, Method::PUT, &format!("/api/lists/payment_methods/{}", m["id"].as_str().unwrap()), t, Some(json!({
            "name": m["name"], "params": {"kind": m["params"]["kind"], "commission": 0.0, "wallets": {store.clone(): w}}
        }))).await;
        assert_eq!(st, StatusCode::OK, "{r}");
    }
    // некорректный счёт отклоняется
    let (st, _) = call(&router, Method::PUT, &format!("/api/lists/payment_methods/{}", cash["id"].as_str().unwrap()), t, Some(json!({
        "name": "Наличные", "params": {"kind": "cash", "wallets": {store.clone(): 5}}
    }))).await;
    assert_eq!(st, StatusCode::BAD_REQUEST);

    let (_, milk) = call(&router, Method::POST, "/api/products", t, Some(json!({
        "name": "Молоко", "purchasePrice": 50.0, "stores": [{"storeId": store, "active": true, "salePrice": 100.0}]
    }))).await;
    let milk = milk["id"].as_str().unwrap().to_string();
    call(&router, Method::PUT, &format!("/api/stocks/{milk}"), t, Some(json!({"storeId": store, "qty": 50.0}))).await;

    // 2 × 100 наличными → касса +200
    let (st, sale) = call(&router, Method::POST, "/api/sales", t, Some(json!({
        "storeId": store, "paymentMethodId": cash["id"], "items": [{"productId": milk, "qty": 2.0}]
    }))).await;
    assert_eq!(st, StatusCode::OK, "{sale}");
    let sale_id = sale["id"].as_str().unwrap().to_string();
    let (_, w) = call(&router, Method::GET, "/api/wallets", t, None).await;
    assert_eq!(balance(&w, &cash_w), 200.0);

    // смешанная оплата 300: 100 наличными + 200 QR
    let (st, mixed) = call(&router, Method::POST, "/api/sales", t, Some(json!({
        "storeId": store, "paymentMethodId": cash["id"], "secondPaymentMethodId": qr["id"],
        "firstPaymentValue": 100.0, "secondPaymentValue": 200.0, "items": [{"productId": milk, "qty": 3.0}]
    }))).await;
    assert_eq!(st, StatusCode::OK, "{mixed}");
    let mixed_id = mixed["id"].as_str().unwrap().to_string();
    let (_, w) = call(&router, Method::GET, "/api/wallets", t, None).await;
    assert_eq!(balance(&w, &cash_w), 300.0);
    assert_eq!(balance(&w, &bank_w), 200.0);

    // возврат 1 шт. из первой продажи (100) → касса −100
    let (_, full) = call(&router, Method::GET, &format!("/api/sales/{sale_id}"), t, None).await;
    let item = full["items"][0]["id"].as_str().unwrap().to_string();
    let (st, r) = call(&router, Method::POST, &format!("/api/sales/{sale_id}/returns"), t, Some(json!({
        "items": [{"saleItemId": item, "qty": 1.0}], "returnType": "money", "restock": true
    }))).await;
    assert_eq!(st, StatusCode::OK, "{r}");
    let (_, w) = call(&router, Method::GET, "/api/wallets", t, None).await;
    assert_eq!(balance(&w, &cash_w), 200.0);

    // удаление смешанной продажи снимает её зачисления
    let (st, _) = call(&router, Method::DELETE, &format!("/api/sales/{mixed_id}"), t, None).await;
    assert_eq!(st, StatusCode::OK);
    let (_, w) = call(&router, Method::GET, "/api/wallets", t, None).await;
    assert_eq!(balance(&w, &cash_w), 100.0);
    assert_eq!(balance(&w, &bank_w), 0.0);

    // удаление первой продажи (с возвратом) обнуляет кассу
    let (st, _) = call(&router, Method::DELETE, &format!("/api/sales/{sale_id}"), t, None).await;
    assert_eq!(st, StatusCode::OK);
    let (_, w) = call(&router, Method::GET, "/api/wallets", t, None).await;
    assert_eq!(balance(&w, &cash_w), 0.0);
}
