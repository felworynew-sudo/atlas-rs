//! Сквозной тест продажи: списки, клиенты, остатки, продажа со скидкой/кэшбэком/смешанной оплатой,
//! идемпотентность, права, возврат, удаление с возвратом остатков, отложенные чеки.
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

fn eq(v: &Value, x: f64) {
    let got = v.as_f64().unwrap_or_else(|| panic!("not a number: {v}"));
    assert!((got - x).abs() < 1e-6, "expected {x}, got {got}");
}

fn s(v: &Value) -> String {
    v.as_str().unwrap().to_string()
}

async fn milk_qty(env: &Env, t: &str, store: &str, milk: &str) -> f64 {
    let (st, rows) = call(env, Method::GET, &format!("/api/stocks?storeId={store}"), Some(t), None).await;
    assert_eq!(st, StatusCode::OK);
    rows.as_array().unwrap().iter().find(|r| r["productId"] == milk).unwrap()["qty"].as_f64().unwrap()
}

#[tokio::test]
async fn sale_flow() {
    let env = setup().await;
    let (st, body) = call(&env, Method::POST, "/api/auth/signup", None, Some(json!({
        "email": "sale@example.com", "password": "secret1", "name": "Owner", "organizationName": "Cafe", "storeName": "Зал 1"
    }))).await;
    assert_eq!(st, StatusCode::OK, "{body}");
    let t = s(&body["token"]);
    let t = t.as_str();

    // настраиваемые списки: значения по умолчанию, валидация, неизвестный вид
    let (_, pm) = call(&env, Method::GET, "/api/lists/payment_methods", Some(t), None).await;
    let names: Vec<&str> = pm.as_array().unwrap().iter().map(|x| x["name"].as_str().unwrap()).collect();
    assert_eq!(names, vec!["Наличные", "QR", "Терминал"]);
    let cash = s(&pm[0]["id"]);
    let qr = s(&pm[1]["id"]);
    let (_, pu) = call(&env, Method::GET, "/api/lists/purchase_methods", Some(t), None).await;
    assert_eq!(pu.as_array().unwrap().len(), 2);
    let (_, dc) = call(&env, Method::GET, "/api/lists/discounts", Some(t), None).await;
    assert_eq!(dc.as_array().unwrap().len(), 10);
    let (st, _) = call(&env, Method::POST, "/api/lists/discounts", Some(t), Some(json!({"name": "Плохая", "params": {"type": "percent", "value": 150}}))).await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
    let (st, _) = call(&env, Method::POST, "/api/lists/discounts", Some(t), Some(json!({"name": "Акция", "params": {"type": "amount", "value": 50}}))).await;
    assert_eq!(st, StatusCode::OK);
    let (st, _) = call(&env, Method::GET, "/api/lists/nope", Some(t), None).await;
    assert_eq!(st, StatusCode::NOT_FOUND);

    // каталог
    let (_, stores) = call(&env, Method::GET, "/api/stores", Some(t), None).await;
    let store = s(&stores[0]["id"]);
    let sp = json!([{ "storeId": store, "salePrice": 100.0 }]);
    let (st, milk) = call(&env, Method::POST, "/api/products", Some(t), Some(json!({"name": "Молоко", "purchasePrice": 60.0, "stores": sp}))).await;
    assert_eq!(st, StatusCode::OK, "{milk}");
    let milk = s(&milk["id"]);
    let (_, latte) = call(&env, Method::POST, "/api/products", Some(t), Some(json!({
        "name": "Латте", "isComposite": true, "productionType": "on_sale",
        "stores": [{"storeId": store, "salePrice": 250.0}], "ingredients": [{"productId": milk, "qty": 0.2}]
    }))).await;
    let latte = s(&latte["id"]);
    let (_, svc) = call(&env, Method::POST, "/api/products", Some(t), Some(json!({
        "name": "Упаковка", "isService": true, "stores": [{"storeId": store, "salePrice": 300.0}]
    }))).await;
    let svc = s(&svc["id"]);

    // остаток вручную: 10 шт; нецелое — нельзя
    let (st, _) = call(&env, Method::PUT, &format!("/api/stocks/{milk}"), Some(t), Some(json!({"storeId": store, "qty": 10.5}))).await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
    let (st, _) = call(&env, Method::PUT, &format!("/api/stocks/{milk}"), Some(t), Some(json!({"storeId": store, "qty": 10}))).await;
    assert_eq!(st, StatusCode::OK);
    let (st, _) = call(&env, Method::PUT, &format!("/api/stocks/{svc}"), Some(t), Some(json!({"storeId": store, "qty": 1}))).await;
    assert_eq!(st, StatusCode::BAD_REQUEST); // у услуги нет остатков

    // клиенты
    let (st, client) = call(&env, Method::POST, "/api/clients", Some(t), Some(json!({"name": "Анна", "phone": "+7 (900) 111-22-33"}))).await;
    assert_eq!(st, StatusCode::OK, "{client}");
    assert_eq!(client["phone"], "+79001112233");
    let client_id = s(&client["id"]);
    let (st, _) = call(&env, Method::POST, "/api/clients", Some(t), Some(json!({"name": "Дубль", "phone": "+79001112233"}))).await;
    assert_eq!(st, StatusCode::CONFLICT);
    let (_, found) = call(&env, Method::GET, "/api/clients?search=1112233", Some(t), None).await;
    assert_eq!(found.as_array().unwrap().len(), 1);

    // продажа №1: скидка 10%, кэшбэк 3% по умолчанию, смешанная оплата
    let sale_body = |key: &str| json!({
        "storeId": store, "clientId": client_id, "idempotencyKey": key,
        "items": [
            {"productId": milk, "qty": 2}, {"productId": latte, "qty": 1}, {"productId": svc, "qty": 1}
        ],
        "discountPercentage": 10.0,
        "paymentMethodId": cash, "secondPaymentMethodId": qr, "firstPaymentValue": 400.0, "secondPaymentValue": 275.0
    });
    let (st, sale1) = call(&env, Method::POST, "/api/sales", Some(t), Some(sale_body("k1"))).await;
    assert_eq!(st, StatusCode::OK, "{sale1}");
    let sale1_id = s(&sale1["id"]);
    eq(&sale1["subtotal"], 750.0);
    eq(&sale1["discountAmount"], 75.0);
    eq(&sale1["total"], 675.0);
    eq(&sale1["bonusAccumulation"], 20.25);
    eq(&sale1["costTotal"], 132.0); // 2*60 + латте 0.2*60
    assert_eq!(sale1["items"].as_array().unwrap().len(), 3);
    eq(&milk_qty(&env, t, &store, &milk).await.into(), 7.8); // 10 - 2 - 0.2 (ингредиент латте)
    let (_, c) = call(&env, Method::GET, &format!("/api/clients/{client_id}"), Some(t), None).await;
    eq(&c["bonusBalance"], 20.25);

    // идемпотентность: тот же ключ не создаёт вторую продажу и не списывает остаток
    let (st, again) = call(&env, Method::POST, "/api/sales", Some(t), Some(sale_body("k1"))).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(again["id"], sale1["id"]);
    eq(&milk_qty(&env, t, &store, &milk).await.into(), 7.8);

    // валидации
    let bad = |extra: Value| {
        let mut b = json!({"storeId": store, "items": [{"productId": milk, "qty": 1}]});
        for (k, v) in extra.as_object().unwrap() {
            b[k] = v.clone();
        }
        b
    };
    for body in [
        bad(json!({"items": []})),
        bad(json!({"items": [{"productId": milk, "qty": 1.5}]})),          // нецелое
        bad(json!({"items": [{"productId": milk, "qty": 0}]})),            // ноль
        bad(json!({"discountAmount": 400.0})),                             // больше чека
        bad(json!({"discountAmount": 10.0, "discountPercentage": 5.0})),   // две скидки
        bad(json!({"bonusReduction": 5.0})),                               // без клиента
        bad(json!({"clientId": client_id, "bonusReduction": 999.0})),      // больше баланса
        bad(json!({"secondPaymentMethodId": qr, "firstPaymentValue": 10.0, "secondPaymentValue": 10.0})),
        bad(json!({"items": [{"name": "Свой", "qty": 1}]})),               // без цены
    ] {
        let (st, e) = call(&env, Method::POST, "/api/sales", Some(t), Some(body.clone())).await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "{body} -> {e}");
    }
    eq(&milk_qty(&env, t, &store, &milk).await.into(), 7.8); // неудачные продажи ничего не списали

    // продажа №2: оплата бонусами 20 → итог 80, кэшбэк 3% от 80 = 2.4
    let (st, sale2) = call(&env, Method::POST, "/api/sales", Some(t), Some(json!({
        "storeId": store, "clientId": client_id, "items": [{"productId": milk, "qty": 1}], "bonusReduction": 20.0
    }))).await;
    assert_eq!(st, StatusCode::OK, "{sale2}");
    eq(&sale2["total"], 80.0);
    eq(&sale2["bonusAccumulation"], 2.4);
    let (_, c) = call(&env, Method::GET, &format!("/api/clients/{client_id}"), Some(t), None).await;
    eq(&c["bonusBalance"], 2.65);
    eq(&milk_qty(&env, t, &store, &milk).await.into(), 6.8);

    // продавец: права и остатки
    let (_, roles) = call(&env, Method::GET, "/api/roles", Some(t), None).await;
    let cashier_role = s(&roles.as_array().unwrap().iter().find(|r| r["key"] == "cashier").unwrap()["id"]);
    let (st, _) = call(&env, Method::POST, "/api/users", Some(t), Some(json!({
        "name": "S", "email": "seller-sale@example.com", "password": "pass123", "roleId": cashier_role, "storeIds": [store]
    }))).await;
    assert_eq!(st, StatusCode::OK);
    let (_, login) = call(&env, Method::POST, "/api/auth/login", None, Some(json!({"email": "seller-sale@example.com", "password": "pass123"}))).await;
    let ct = s(&login["token"]);
    let ct = ct.as_str();
    let (st, e) = call(&env, Method::POST, "/api/sales", Some(ct), Some(json!({"storeId": store, "items": [{"productId": milk, "qty": 100}]}))).await;
    assert_eq!(st, StatusCode::BAD_REQUEST, "{e}");
    assert!(e["message"].as_str().unwrap().starts_with("Недостаточно товара"));
    let (st, _) = call(&env, Method::POST, "/api/sales", Some(ct), Some(json!({"storeId": store, "discountPercentage": 5.0, "items": [{"productId": milk, "qty": 1}]}))).await;
    assert_eq!(st, StatusCode::FORBIDDEN); // нет права «Применять скидки»
    let (st, cs) = call(&env, Method::POST, "/api/sales", Some(ct), Some(json!({"storeId": store, "items": [{"name": "Подарок", "price": 50.0, "qty": 1}]}))).await;
    assert_eq!(st, StatusCode::OK, "{cs}"); // кастомная позиция разрешена продавцу
    assert!(cs["items"][0]["productId"].is_null());
    let (st, _) = call(&env, Method::DELETE, &format!("/api/sales/{sale1_id}"), Some(ct), None).await;
    assert_eq!(st, StatusCode::FORBIDDEN);

    // возврат: часть продажи №1, деньги, на склад
    let milk_item = s(&sale1["items"].as_array().unwrap().iter().find(|i| i["productId"] == milk).unwrap()["id"]);
    let (st, r) = call(&env, Method::POST, &format!("/api/sales/{sale1_id}/returns"), Some(t), Some(json!({
        "items": [{"saleItemId": milk_item, "qty": 1}], "returnType": "money", "restock": true
    }))).await;
    assert_eq!(st, StatusCode::OK, "{r}");
    eq(&r["amount"], 90.0); // 100 * 675/750
    eq(&milk_qty(&env, t, &store, &milk).await.into(), 7.8);
    let (st, _) = call(&env, Method::POST, &format!("/api/sales/{sale1_id}/returns"), Some(t), Some(json!({
        "items": [{"saleItemId": milk_item, "qty": 2}], "returnType": "money", "restock": true
    }))).await;
    assert_eq!(st, StatusCode::BAD_REQUEST); // осталось вернуть 1, а не 2
    let (_, got) = call(&env, Method::GET, &format!("/api/sales/{sale1_id}"), Some(t), None).await;
    eq(&got["returnedAmount"], 90.0);

    // удаление продажи №1: остатки и бонусы откатываются
    let (st, _) = call(&env, Method::DELETE, &format!("/api/sales/{sale1_id}"), Some(t), None).await;
    assert_eq!(st, StatusCode::OK);
    eq(&milk_qty(&env, t, &store, &milk).await.into(), 9.0); // 7.8 + (2 + 0.2 - 1)
    let (_, c) = call(&env, Method::GET, &format!("/api/clients/{client_id}"), Some(t), None).await;
    eq(&c["bonusBalance"], 0.0);
    let (st, _) = call(&env, Method::GET, &format!("/api/sales/{sale1_id}"), Some(t), None).await;
    assert_eq!(st, StatusCode::NOT_FOUND);
    let (_, list) = call(&env, Method::GET, &format!("/api/sales?storeId={store}"), Some(t), None).await;
    assert_eq!(list.as_array().unwrap().len(), 2); // продажа №2 и продажа продавца

    // история движения молока
    let (st, mv) = call(&env, Method::GET, &format!("/api/stocks/{milk}/movements?storeId={store}"), Some(t), None).await;
    assert_eq!(st, StatusCode::OK);
    let kinds: Vec<&str> = mv.as_array().unwrap().iter().map(|m| m["kind"].as_str().unwrap()).collect();
    for k in ["manual", "sale", "return", "sale_deleted"] {
        assert!(kinds.contains(&k), "нет движения {k}: {kinds:?}");
    }
    assert_eq!(kinds[0], "sale_deleted"); // новые сверху

    // отложенные чеки
    let (st, cart) = call(&env, Method::POST, "/api/saved-carts", Some(ct), Some(json!({"storeId": store, "name": "Стол 3", "payload": {"items": [{"productId": milk, "qty": 1}]}}))).await;
    assert_eq!(st, StatusCode::OK, "{cart}");
    let (_, carts) = call(&env, Method::GET, &format!("/api/saved-carts?storeId={store}"), Some(ct), None).await;
    assert_eq!(carts.as_array().unwrap().len(), 1);
    let (st, _) = call(&env, Method::DELETE, &format!("/api/saved-carts/{}", s(&cart["id"])), Some(ct), None).await;
    assert_eq!(st, StatusCode::OK);

    // без токена
    let (st, _) = call(&env, Method::POST, "/api/sales", None, Some(json!({}))).await;
    assert_eq!(st, StatusCode::UNAUTHORIZED);
}
