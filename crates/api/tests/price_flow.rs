//! Ценовые категории: создание, авто-расчёт и ручные цены, предпочитаемая категория клиента, продажа с типом цены.
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

async fn call(router: &Router, method: Method, uri: &str, token: &str, body: Option<Value>) -> (StatusCode, Value) {
    let req = Request::builder().method(method).uri(uri).header(header::AUTHORIZATION, format!("Bearer {token}"));
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
async fn price_types_flow() {
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

    let req = Request::builder()
        .method(Method::POST)
        .uri("/api/auth/signup")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(json!({"email": "p@example.com", "password": "secret1", "name": "P", "organizationName": "Shop", "storeName": "Точка"}).to_string()))
        .unwrap();
    let resp = router.clone().oneshot(req).await.unwrap();
    let body: Value = serde_json::from_slice(&resp.into_body().collect().await.unwrap().to_bytes()).unwrap();
    let t = body["token"].as_str().unwrap().to_string();
    let t = t.as_str();

    let (_, stores) = call(&router, Method::GET, "/api/stores", t, None).await;
    let store = stores[0]["id"].as_str().unwrap().to_string();
    let (st, milk) = call(&router, Method::POST, "/api/products", t, Some(json!({
        "name": "Молоко", "purchasePrice": 50.0, "stores": [{"storeId": store, "active": true, "salePrice": 100.0}]
    }))).await;
    assert_eq!(st, StatusCode::OK, "{milk}");
    let milk_id = milk["id"].as_str().unwrap().to_string();

    // авто-расчёт: +10%
    let (st, auto) = call(&router, Method::POST, "/api/price-types", t, Some(json!({"name": "Опт", "autoCalc": true, "markupPercent": -10.0, "rules": {"minQty": 5}}))).await;
    assert_eq!(st, StatusCode::OK, "{auto}");
    let auto_id = auto["id"].as_str().unwrap().to_string();
    let (_, eff) = call(&router, Method::GET, &format!("/api/price-types/{auto_id}/effective?storeId={store}"), t, None).await;
    assert_eq!(eff[&milk_id], 90.0);

    // ручная цена перекрывает авто-расчёт, удаление возвращает расчёт
    let (st, _) = call(&router, Method::PUT, &format!("/api/price-types/{auto_id}/prices"), t, Some(json!([{"productId": milk_id, "storeId": store, "price": 80.0}]))).await;
    assert_eq!(st, StatusCode::OK);
    let (_, eff) = call(&router, Method::GET, &format!("/api/price-types/{auto_id}/effective?storeId={store}"), t, None).await;
    assert_eq!(eff[&milk_id], 80.0);
    call(&router, Method::PUT, &format!("/api/price-types/{auto_id}/prices"), t, Some(json!([{"productId": milk_id, "storeId": store, "price": null}]))).await;
    let (_, eff) = call(&router, Method::GET, &format!("/api/price-types/{auto_id}/effective?storeId={store}"), t, None).await;
    assert_eq!(eff[&milk_id], 90.0);

    // неверные правила
    let (st, _) = call(&router, Method::POST, "/api/price-types", t, Some(json!({"name": "X", "rules": {"bogus": 1}}))).await;
    assert_eq!(st, StatusCode::BAD_REQUEST);

    // клиент с предпочитаемой категорией
    let (_, client) = call(&router, Method::POST, "/api/clients", t, Some(json!({"name": "Оптовик"}))).await;
    let cid = client["id"].as_str().unwrap().to_string();
    let (st, _) = call(&router, Method::PUT, &format!("/api/price-types/client/{cid}"), t, Some(json!({"priceTypeId": auto_id}))).await;
    assert_eq!(st, StatusCode::OK);
    let (_, c) = call(&router, Method::GET, &format!("/api/clients/{cid}"), t, None).await;
    assert_eq!(c["preferredPriceTypeId"], auto_id);

    // продажа по цене категории; тип цены сохраняется в продаже
    call(&router, Method::PUT, &format!("/api/stocks/{milk_id}"), t, Some(json!({"storeId": store, "qty": 10.0}))).await;
    let (st, sale) = call(&router, Method::POST, "/api/sales", t, Some(json!({
        "storeId": store, "clientId": cid, "priceTypeId": auto_id, "items": [{"productId": milk_id, "qty": 2.0, "price": 90.0}]
    }))).await;
    assert_eq!(st, StatusCode::OK, "{sale}");
    assert_eq!(sale["total"], 180.0);
    let (st, _) = call(&router, Method::POST, "/api/sales", t, Some(json!({
        "storeId": store, "priceTypeId": "00000000-0000-0000-0000-000000000001", "items": [{"productId": milk_id, "qty": 1.0}]
    }))).await;
    assert_eq!(st, StatusCode::BAD_REQUEST);

    // удаление категории сбрасывает предпочтение клиента
    let (st, _) = call(&router, Method::DELETE, &format!("/api/price-types/{auto_id}"), t, None).await;
    assert_eq!(st, StatusCode::OK);
    let (_, c) = call(&router, Method::GET, &format!("/api/clients/{cid}"), t, None).await;
    assert!(c["preferredPriceTypeId"].is_null());
}
