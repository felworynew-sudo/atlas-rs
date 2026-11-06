//! Сквозной тест складских документов: приёмка (с возвратом поставщику и обновлением цен), заказы поставщикам,
//! списание, ревизия, сторно при удалении, сводка и предложение к заказу.
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
    sqlx::raw_sql("DROP SCHEMA public CASCADE; CREATE SCHEMA public;").execute(&pool).await.unwrap();
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

fn s(v: &Value) -> String {
    v.as_str().unwrap().to_string()
}

fn eq(v: &Value, x: f64) {
    let got = v.as_f64().unwrap_or_else(|| panic!("not a number: {v}"));
    assert!((got - x).abs() < 1e-6, "expected {x}, got {got}");
}

async fn qty(env: &Env, t: &str, store: &str, product: &str) -> f64 {
    let (_, rows) = call(env, Method::GET, &format!("/api/stocks?storeId={store}"), Some(t), None).await;
    rows.as_array().unwrap().iter().find(|r| r["productId"] == product).unwrap()["qty"].as_f64().unwrap()
}

#[tokio::test]
async fn stock_flow() {
    let env = setup().await;
    let (st, body) = call(&env, Method::POST, "/api/auth/signup", None, Some(json!({
        "email": "stock@example.com", "password": "secret1", "name": "Owner", "organizationName": "Shop", "storeName": "Склад"
    }))).await;
    assert_eq!(st, StatusCode::OK, "{body}");
    let t = s(&body["token"]);
    let t = t.as_str();
    let (_, stores) = call(&env, Method::GET, "/api/stores", Some(t), None).await;
    let store = s(&stores[0]["id"]);
    let (_, sup) = call(&env, Method::POST, "/api/suppliers", Some(t), Some(json!({"name": "Опт"}))).await;
    let sup = s(&sup["id"]);
    let sp = json!([{ "storeId": store, "salePrice": 100.0 }]);
    let (_, milk) = call(&env, Method::POST, "/api/products", Some(t), Some(json!({"name": "Молоко", "purchasePrice": 60.0, "supplierId": sup, "stores": sp}))).await;
    let milk = s(&milk["id"]);
    let (_, cheese) = call(&env, Method::POST, "/api/products", Some(t), Some(json!({"name": "Сыр", "purchasePrice": 200.0, "supplierId": sup, "stores": sp}))).await;
    let cheese = s(&cheese["id"]);
    let (_, svc) = call(&env, Method::POST, "/api/products", Some(t), Some(json!({"name": "Услуга", "isService": true}))).await;
    let svc = s(&svc["id"]);

    // приёмка: остаток растёт, закупочная цена обновляется
    let (st, sy) = call(&env, Method::POST, "/api/supplies", Some(t), Some(json!({
        "storeId": store, "supplierId": sup, "pricesChanged": true, "paid": true,
        "items": [{"productId": milk, "qty": 20, "price": 70.0}]
    }))).await;
    assert_eq!(st, StatusCode::OK, "{sy}");
    eq(&sy["total"], 1400.0);
    assert_eq!(qty(&env, t, &store, &milk).await, 20.0);
    let (_, p) = call(&env, Method::GET, &format!("/api/products/{milk}"), Some(t), None).await;
    eq(&p["purchasePrice"], 70.0);

    // валидации приёмки
    for items in [
        json!([]),
        json!([{"productId": milk, "qty": 0, "price": 1.0}]),
        json!([{"productId": milk, "qty": 1.5, "price": 1.0}]),
        json!([{"productId": milk, "qty": 1, "price": -1.0}]),
        json!([{"productId": svc, "qty": 1, "price": 1.0}]),
        json!([{"productId": milk, "qty": 1, "price": 1.0}, {"productId": milk, "qty": 1, "price": 1.0}]),
    ] {
        let (st, e) = call(&env, Method::POST, "/api/supplies", Some(t), Some(json!({"storeId": store, "items": items.clone()}))).await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "{items} -> {e}");
    }

    // возврат поставщику: остаток уменьшается, цены не трогаются; больше остатка — нельзя
    let (st, _) = call(&env, Method::POST, "/api/supplies", Some(t), Some(json!({
        "storeId": store, "hasReturn": true, "pricesChanged": true, "items": [{"productId": milk, "qty": 5, "price": 1.0}]
    }))).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(qty(&env, t, &store, &milk).await, 15.0);
    let (_, p) = call(&env, Method::GET, &format!("/api/products/{milk}"), Some(t), None).await;
    eq(&p["purchasePrice"], 70.0);
    let (st, _) = call(&env, Method::POST, "/api/supplies", Some(t), Some(json!({
        "storeId": store, "hasReturn": true, "items": [{"productId": milk, "qty": 100, "price": 1.0}]
    }))).await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
    assert_eq!(qty(&env, t, &store, &milk).await, 15.0);

    // заказ поставщику → приёмки по заказу меняют статус
    let (st, order) = call(&env, Method::POST, "/api/supply-orders", Some(t), Some(json!({
        "storeId": store, "supplierId": sup, "status": "ordered", "items": [{"productId": milk, "qty": 10}]
    }))).await;
    assert_eq!(st, StatusCode::OK, "{order}");
    let order_id = s(&order["id"]);
    eq(&order["total"], 700.0); // цена по умолчанию — закупочная (70)
    let (st, _) = call(&env, Method::POST, "/api/supply-orders", Some(t), Some(json!({"storeId": store, "status": "bogus", "items": [{"productId": milk, "qty": 1}]}))).await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
    let (st, _) = call(&env, Method::POST, "/api/supply-orders", Some(t), Some(json!({"storeId": store, "items": [{"productId": svc, "qty": 1}]}))).await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
    let (_, s4) = call(&env, Method::POST, "/api/supplies", Some(t), Some(json!({
        "storeId": store, "orderId": order_id, "items": [{"productId": milk, "qty": 4, "price": 70.0}]
    }))).await;
    let (_, o) = call(&env, Method::GET, &format!("/api/supply-orders/{order_id}"), Some(t), None).await;
    assert_eq!(o["status"], "partially_received");
    let (_, s6) = call(&env, Method::POST, "/api/supplies", Some(t), Some(json!({
        "storeId": store, "orderId": order_id, "items": [{"productId": milk, "qty": 6, "price": 70.0}]
    }))).await;
    let (_, o) = call(&env, Method::GET, &format!("/api/supply-orders/{order_id}"), Some(t), None).await;
    assert_eq!(o["status"], "received");
    assert_eq!(qty(&env, t, &store, &milk).await, 25.0);
    let (st, _) = call(&env, Method::DELETE, &format!("/api/supplies/{}", s(&s6["id"])), Some(t), None).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(qty(&env, t, &store, &milk).await, 19.0);
    let (_, o) = call(&env, Method::GET, &format!("/api/supply-orders/{order_id}"), Some(t), None).await;
    assert_eq!(o["status"], "partially_received");
    let (_, active) = call(&env, Method::GET, "/api/supply-orders?activeOnly=true", Some(t), None).await;
    assert_eq!(active.as_array().unwrap().len(), 1);
    let _ = s4;

    // списание: причина обязательна, сумма по себестоимости, удаление возвращает остаток
    let (_, reasons) = call(&env, Method::GET, "/api/lists/writeoff_reasons", Some(t), None).await;
    let reason = s(&reasons[0]["id"]);
    let (st, _) = call(&env, Method::POST, "/api/writeoffs", Some(t), Some(json!({"storeId": store, "productId": milk, "qty": 3}))).await;
    assert_eq!(st, StatusCode::BAD_REQUEST); // нет причины
    let (st, w) = call(&env, Method::POST, "/api/writeoffs", Some(t), Some(json!({"storeId": store, "productId": milk, "qty": 3, "reasonId": reason}))).await;
    assert_eq!(st, StatusCode::OK, "{w}");
    eq(&w["amount"], 210.0);
    assert_eq!(qty(&env, t, &store, &milk).await, 16.0);
    let (st, _) = call(&env, Method::DELETE, &format!("/api/writeoffs/{}", s(&w["id"])), Some(t), None).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(qty(&env, t, &store, &milk).await, 19.0);
    let (st, _) = call(&env, Method::POST, "/api/writeoffs", Some(t), Some(json!({"storeId": store, "productId": milk, "qty": 2, "reasonId": reason}))).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(qty(&env, t, &store, &milk).await, 17.0);

    // ревизия: остаток приводится к факту, разница в рублях по себестоимости
    let (st, rev) = call(&env, Method::POST, "/api/revisions", Some(t), Some(json!({
        "storeId": store, "items": [{"productId": milk, "factQty": 20}]
    }))).await;
    assert_eq!(st, StatusCode::OK, "{rev}");
    eq(&rev["totalDiff"], 210.0);
    eq(&rev["items"][0]["systemQty"], 17.0);
    assert_eq!(qty(&env, t, &store, &milk).await, 20.0);
    let (_, rev2) = call(&env, Method::POST, "/api/revisions", Some(t), Some(json!({
        "storeId": store, "items": [{"productId": milk, "factQty": 18}]
    }))).await;
    eq(&rev2["totalDiff"], -140.0);
    assert_eq!(qty(&env, t, &store, &milk).await, 18.0);
    let (st, _) = call(&env, Method::DELETE, &format!("/api/revisions/{}", s(&rev2["id"])), Some(t), None).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(qty(&env, t, &store, &milk).await, 20.0);
    let (st, _) = call(&env, Method::POST, "/api/revisions", Some(t), Some(json!({"storeId": store, "items": [{"productId": svc, "factQty": 1}]}))).await;
    assert_eq!(st, StatusCode::BAD_REQUEST); // у услуги нет остатков

    // история движения и показатели за 7 дней
    let (_, mv) = call(&env, Method::GET, &format!("/api/stocks/{milk}/movements?storeId={store}"), Some(t), None).await;
    let kinds: Vec<&str> = mv.as_array().unwrap().iter().map(|m| m["kind"].as_str().unwrap()).collect();
    for k in ["supply", "writeoff", "revision"] {
        assert!(kinds.contains(&k), "нет движения {k}: {kinds:?}");
    }
    let (_, rows) = call(&env, Method::GET, &format!("/api/stocks?storeId={store}"), Some(t), None).await;
    let m = rows.as_array().unwrap().iter().find(|r| r["productId"] == milk.as_str()).unwrap();
    eq(&m["writtenOff7"], 2.0);
    assert_eq!(m["low"], false);

    // сводка по складу: 20 шт × 70 по закупке, × 100 по продаже
    let (st, sm) = call(&env, Method::GET, &format!("/api/stocks/summary?storeId={store}"), Some(t), None).await;
    assert_eq!(st, StatusCode::OK);
    eq(&sm["unitsInStock"], 20.0);
    eq(&sm["valueAtPurchase"], 1400.0);
    eq(&sm["valueAtSale"], 2000.0);

    // предложение к заказу: сыр (остаток 0 ≤ порога 1) → 2 шт; молоко не предлагается
    let (st, sg) = call(&env, Method::GET, &format!("/api/supply-orders/suggest?storeId={store}&supplierId={sup}"), Some(t), None).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(sg.as_array().unwrap().len(), 1);
    assert_eq!(sg[0]["productId"], cheese.as_str());
    eq(&sg[0]["suggestedQty"], 2.0);

    // права: продавец видит остатки, но не создаёт и не удаляет складские документы
    let (_, roles) = call(&env, Method::GET, "/api/roles", Some(t), None).await;
    let cashier = s(&roles.as_array().unwrap().iter().find(|r| r["key"] == "cashier").unwrap()["id"]);
    call(&env, Method::POST, "/api/users", Some(t), Some(json!({
        "name": "S", "email": "seller-stock@example.com", "password": "pass123", "roleId": cashier, "storeIds": [store]
    }))).await;
    let (_, login) = call(&env, Method::POST, "/api/auth/login", None, Some(json!({"email": "seller-stock@example.com", "password": "pass123"}))).await;
    let ct = s(&login["token"]);
    let ct = ct.as_str();
    let (st, _) = call(&env, Method::GET, &format!("/api/stocks?storeId={store}"), Some(ct), None).await;
    assert_eq!(st, StatusCode::OK);
    // приёмку продавец создать может (право «История приемок»), но не удалить
    let (st, _) = call(&env, Method::POST, "/api/supplies", Some(ct), Some(json!({"storeId": store, "items": [{"productId": milk, "qty": 1, "price": 1.0}]}))).await;
    assert_eq!(st, StatusCode::OK);
    let (st, _) = call(&env, Method::DELETE, &format!("/api/supplies/{}", s(&s4["id"])), Some(ct), None).await;
    assert_eq!(st, StatusCode::FORBIDDEN);
    // ревизию продавец делать может (право «Ревизии»), а удалять — нет
    let (st, r) = call(&env, Method::POST, "/api/revisions", Some(ct), Some(json!({"storeId": store, "items": [{"productId": milk, "factQty": 20}]}))).await;
    assert_eq!(st, StatusCode::OK, "{r}");
    let (st, _) = call(&env, Method::DELETE, &format!("/api/revisions/{}", s(&r["id"])), Some(ct), None).await;
    assert_eq!(st, StatusCode::FORBIDDEN);
}
