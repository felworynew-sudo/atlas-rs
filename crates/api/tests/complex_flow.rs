//! Сквозной тест шага 6: модули организации, лояльность (кэшбэк, карта «N-й в подарок», бонусы),
//! доставка, комплексные продажи, заказы клиентов и онлайн-каталог.
use atlas_api::{app, AppState, MIGRATOR};
use axum::{
    body::Body,
    http::{header, Method, Request, StatusCode},
    Router,
};
use chrono::{Datelike, NaiveDate, Utc};
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
    (status, if bytes.is_empty() { Value::Null } else { serde_json::from_slice(&bytes).unwrap_or(Value::Null) })
}

fn s(v: &Value) -> String {
    v.as_str().unwrap_or_else(|| panic!("not a string: {v}")).to_string()
}

fn eq(v: &Value, x: f64) {
    let got = v.as_f64().unwrap_or_else(|| panic!("not a number: {v}"));
    assert!((got - x).abs() < 0.005, "expected {x}, got {got}");
}

async fn qty(env: &Env, t: &str, store: &str, product: &str) -> f64 {
    let (_, rows) = call(env, Method::GET, &format!("/api/stocks?storeId={store}"), Some(t), None).await;
    rows.as_array().unwrap().iter().find(|r| r["productId"] == product).unwrap()["qty"].as_f64().unwrap()
}

#[tokio::test]
async fn complex_flow() {
    let env = setup().await;
    let (st, body) = call(&env, Method::POST, "/api/auth/signup", None, Some(json!({
        "email": "cx@example.com", "password": "secret1", "name": "Owner", "organizationName": "Studio", "storeName": "Зал"
    }))).await;
    assert_eq!(st, StatusCode::OK, "{body}");
    let t = s(&body["token"]);
    let t = t.as_str();
    let (_, stores) = call(&env, Method::GET, "/api/stores", Some(t), None).await;
    let store = s(&stores[0]["id"]);
    let (_, st2) = call(&env, Method::POST, "/api/stores", Some(t), Some(json!({"name": "Мастерская"}))).await;
    let store2 = s(&st2["id"]);

    // выключенный модуль «Приемки»: право не действует даже у админа, после включения — работает
    let (st, _) = call(&env, Method::GET, "/api/supplies", Some(t), None).await;
    assert_eq!(st, StatusCode::FORBIDDEN);
    let (_, org) = call(&env, Method::GET, "/api/organizations", Some(t), None).await;
    let mut modules = org["modules"].clone();
    modules["supplies"] = json!(true);
    call(&env, Method::PUT, &format!("/api/organizations/{}", s(&org["id"])), Some(t), Some(json!({"modules": modules}))).await;
    let (st, _) = call(&env, Method::GET, "/api/supplies", Some(t), None).await;
    assert_eq!(st, StatusCode::OK);

    // товар и остатки
    let (_, milk) = call(&env, Method::POST, "/api/products", Some(t), Some(json!({
        "name": "Молоко", "purchasePrice": 60.0,
        "stores": [{"storeId": store, "salePrice": 100.0}, {"storeId": store2, "salePrice": 100.0}]
    }))).await;
    let milk = s(&milk["id"]);
    call(&env, Method::PUT, &format!("/api/stocks/{milk}"), Some(t), Some(json!({"storeId": store, "qty": 100}))).await;
    call(&env, Method::PUT, &format!("/api/stocks/{milk}"), Some(t), Some(json!({"storeId": store2, "qty": 10}))).await;

    // настройки лояльности
    let (_, loy) = call(&env, Method::GET, "/api/loyalty-settings", Some(t), None).await;
    eq(&loy["defaultBonusLevel"], 1.0);
    assert_eq!(loy["punchCardEnabled"], false);
    for bad in [json!({"punchCardCapacity": 1}), json!({"nope": true}), json!({"cashbackEnabled": "yes"})] {
        let (st, _) = call(&env, Method::PATCH, "/api/loyalty-settings", Some(t), Some(bad)).await;
        assert_eq!(st, StatusCode::BAD_REQUEST);
    }
    let (st, loy) = call(&env, Method::PATCH, "/api/loyalty-settings", Some(t), Some(json!({"punchCardEnabled": true, "punchCardCapacity": 3, "defaultBonusLevel": 2}))).await;
    assert_eq!(st, StatusCode::OK, "{loy}");

    // клиент: телефон обязан начинаться на «+», уровень бонусов — из настроек лояльности
    let (st, e) = call(&env, Method::POST, "/api/clients", Some(t), Some(json!({"name": "Анна", "phone": "79001112233"}))).await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
    assert_eq!(e["message"], "Номер телефона должен начинаться на +");
    let (st, client) = call(&env, Method::POST, "/api/clients", Some(t), Some(json!({"name": "Анна", "phone": "+79001112233", "address": "ул. Мира, 1"}))).await;
    assert_eq!(st, StatusCode::OK, "{client}");
    eq(&client["bonusLevel"], 2.0);
    let cid = s(&client["id"]);

    // карта «3-й в подарок»: 2 шт. — отметки, во второй покупке 3-я единица бесплатна
    let (_, a) = call(&env, Method::POST, "/api/sales", Some(t), Some(json!({"storeId": store, "clientId": cid, "items": [{"productId": milk, "qty": 2}]}))).await;
    eq(&a["total"], 200.0);
    eq(&a["punchCardReduction"], 0.0);
    let (_, b) = call(&env, Method::POST, "/api/sales", Some(t), Some(json!({"storeId": store, "clientId": cid, "items": [{"productId": milk, "qty": 2}]}))).await;
    eq(&b["punchCardReduction"], 100.0);
    eq(&b["total"], 100.0);
    let (_, pc) = call(&env, Method::GET, &format!("/api/clients/{cid}/punch-card"), Some(t), None).await;
    assert_eq!(pc["punches"], 1);
    assert_eq!(pc["capacity"], 3);
    // удаление продажи возвращает отметки
    let (st, _) = call(&env, Method::DELETE, &format!("/api/sales/{}", s(&b["id"])), Some(t), None).await;
    assert_eq!(st, StatusCode::OK);
    let (_, pc) = call(&env, Method::GET, &format!("/api/clients/{cid}/punch-card"), Some(t), None).await;
    assert_eq!(pc["punches"], 2);
    let (st, pc) = call(&env, Method::POST, &format!("/api/clients/{cid}/punch-card/reset"), Some(t), None).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(pc["punches"], 0);

    // ручные бонусы: идемпотентность и защита от ухода в минус
    let (st, adj) = call(&env, Method::POST, &format!("/api/clients/{cid}/bonus-adjustments"), Some(t), Some(json!({"amount": 50.0, "operationId": "op-1", "comment": "Подарок"}))).await;
    assert_eq!(st, StatusCode::OK, "{adj}");
    let after = adj["bonusAfter"].as_f64().unwrap();
    assert!((after - adj["bonusBefore"].as_f64().unwrap() - 50.0).abs() < 0.005);
    let (_, dup) = call(&env, Method::POST, &format!("/api/clients/{cid}/bonus-adjustments"), Some(t), Some(json!({"amount": 50.0, "operationId": "op-1"}))).await;
    assert_eq!(dup["duplicate"], true);
    let (_, c) = call(&env, Method::GET, &format!("/api/clients/{cid}"), Some(t), None).await;
    eq(&c["bonusBalance"], after);
    let (st, _) = call(&env, Method::POST, &format!("/api/clients/{cid}/bonus-adjustments"), Some(t), Some(json!({"amount": -100000.0}))).await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
    let (st, _) = call(&env, Method::POST, &format!("/api/clients/{cid}/bonus-adjustments"), Some(t), Some(json!({"amount": 0}))).await;
    assert_eq!(st, StatusCode::BAD_REQUEST);

    // постоянная скидка клиента применяется автоматически
    let (_, discounts) = call(&env, Method::GET, "/api/lists/discounts", Some(t), None).await;
    let ten = s(&discounts[1]["id"]); // 10%
    let (st, _) = call(&env, Method::PUT, &format!("/api/clients/{cid}"), Some(t), Some(json!({"name": "Анна", "discountId": ten}))).await;
    assert_eq!(st, StatusCode::OK);
    let (_, d) = call(&env, Method::POST, "/api/sales", Some(t), Some(json!({"storeId": store, "clientId": cid, "items": [{"productId": milk, "qty": 1}]}))).await;
    eq(&d["discountAmount"], 10.0);
    eq(&d["total"], 90.0);

    // статистика клиентов и история покупок
    let (_, stats) = call(&env, Method::GET, "/api/clients/stats?days=30", Some(t), None).await;
    assert_eq!(stats["total"], 1);
    assert_eq!(stats["new"], 1);
    assert_eq!(stats["active"], 1);
    let (_, hist) = call(&env, Method::GET, &format!("/api/clients/{cid}/purchase-history"), Some(t), None).await;
    assert_eq!(hist.as_array().unwrap().len(), 2); // продажа b удалена
    let (_, list) = call(&env, Method::GET, &format!("/api/clients?productId={milk}"), Some(t), None).await;
    assert_eq!(list[0]["purchases"], 2);

    // доставка: плата покупателя добавляется к чеку, при оплате продавцом — нет
    let (st, _) = call(&env, Method::PATCH, "/api/sale-settings", Some(t), Some(json!({"deliveryPrice": 150.0}))).await;
    assert_eq!(st, StatusCode::OK);
    let (_, dl) = call(&env, Method::POST, "/api/sales", Some(t), Some(json!({
        "storeId": store, "delivery": true, "deliveryAddress": "ул. Ленина, 5", "items": [{"productId": milk, "qty": 1}]
    }))).await;
    eq(&dl["total"], 250.0);
    eq(&dl["deliveryPrice"], 150.0);
    call(&env, Method::PATCH, "/api/sale-settings", Some(t), Some(json!({"deliveryPaidByCustomer": false}))).await;
    let (_, dl2) = call(&env, Method::POST, "/api/sales", Some(t), Some(json!({"storeId": store, "delivery": true, "items": [{"productId": milk, "qty": 1}]}))).await;
    eq(&dl2["total"], 100.0);

    // ---- комплексные продажи (во второй точке, чтобы P&L был чистым) ----
    let (_, wallet) = call(&env, Method::POST, "/api/wallets", Some(t), Some(json!({"name": "Касса мастерской", "storeId": store2}))).await;
    let wallet = s(&wallet["id"]);
    let (st, tpl) = call(&env, Method::POST, "/api/complex-sale-templates", Some(t), Some(json!({
        "name": "Рамка", "finalPrice": 1000.0, "items": [{"name": "Багет", "qty": 1, "cost": 200}]
    }))).await;
    assert_eq!(st, StatusCode::OK, "{tpl}");
    let (_, tpls) = call(&env, Method::GET, "/api/complex-sale-templates", Some(t), None).await;
    assert_eq!(tpls.as_array().unwrap().len(), 1);

    let body = json!({
        "storeId": store2, "clientId": cid, "templateName": "Рамка", "name": "Рамка для картины", "finalPrice": 1000.0,
        "items": [
            {"productId": milk, "qty": 2},
            {"name": "Багет", "qty": 1, "cost": 200.0, "extraCost": 50.0}
        ],
        "payments": [{"walletId": wallet, "amount": 300.0, "comment": "Предоплата"}]
    });
    let (st, cs) = call(&env, Method::POST, "/api/complex-sales", Some(t), Some(body)).await;
    assert_eq!(st, StatusCode::OK, "{cs}");
    let csid = s(&cs["id"]);
    eq(&cs["expensesTotal"], 370.0); // 2×60 + 200 + 50
    eq(&cs["expectedProfit"], 630.0);
    eq(&cs["paid"], 300.0);
    eq(&cs["remaining"], 700.0);
    assert_eq!(cs["status"], "new");
    assert_eq!(qty(&env, t, &store2, &milk).await, 8.0);
    let w: Value = call(&env, Method::GET, &format!("/api/wallets/{wallet}"), Some(t), None).await.1;
    eq(&w["balance"], 300.0);

    // переплата — нельзя; доплата — можно
    let (st, _) = call(&env, Method::POST, &format!("/api/complex-sales/{csid}/payments"), Some(t), Some(json!({"amount": 800.0}))).await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
    let (st, cs2) = call(&env, Method::POST, &format!("/api/complex-sales/{csid}/payments"), Some(t), Some(json!({"walletId": wallet, "amount": 200.0}))).await;
    assert_eq!(st, StatusCode::OK, "{cs2}");
    eq(&cs2["remaining"], 500.0);
    let pay_id = s(&cs2["payments"][1]["id"]);
    let (st, cs3) = call(&env, Method::DELETE, &format!("/api/complex-sales/{csid}/payments/{pay_id}"), Some(t), None).await;
    assert_eq!(st, StatusCode::OK);
    eq(&cs3["remaining"], 700.0);
    let w: Value = call(&env, Method::GET, &format!("/api/wallets/{wallet}"), Some(t), None).await.1;
    eq(&w["balance"], 300.0);

    // изменение состава и статуса: списание материалов пересчитывается
    let upd = |status: &str, milk_qty: f64| json!({
        "storeId": store2, "clientId": cid, "name": "Рамка для картины", "finalPrice": 1000.0, "status": status,
        "items": [{"productId": milk, "qty": milk_qty}, {"name": "Багет", "qty": 1, "cost": 200.0, "extraCost": 50.0}]
    });
    let (st, v) = call(&env, Method::PUT, &format!("/api/complex-sales/{csid}"), Some(t), Some(upd("in_progress", 3.0))).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(qty(&env, t, &store2, &milk).await, 7.0);
    call(&env, Method::PUT, &format!("/api/complex-sales/{csid}"), Some(t), Some(upd("cancelled", 3.0))).await;
    assert_eq!(qty(&env, t, &store2, &milk).await, 10.0); // отмена вернула материалы
    let (st, _) = call(&env, Method::PUT, &format!("/api/complex-sales/{csid}"), Some(t), Some(upd("bogus", 3.0))).await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
    let (_, v) = call(&env, Method::PUT, &format!("/api/complex-sales/{csid}"), Some(t), Some(upd("done", 3.0))).await;
    assert_eq!(qty(&env, t, &store2, &milk).await, 7.0);
    eq(&v["expensesTotal"], 430.0); // 3×60 + 250
    assert_eq!(v["updates"].as_array().unwrap().len(), 4); // создана + 3 смены статуса
    let (st, _) = call(&env, Method::POST, &format!("/api/complex-sales/{csid}/comments"), Some(t), Some(json!({"text": "Клиент заберёт в пятницу"}))).await;
    assert_eq!(st, StatusCode::OK);
    let (_, active) = call(&env, Method::GET, "/api/complex-sales?activeOnly=true", Some(t), None).await;
    assert_eq!(active.as_array().unwrap().len(), 0); // «Завершён» — не активная

    // завершённая комплексная продажа попадает в P&L точки: выручка 1000, себестоимость 430
    let today = Utc::now().date_naive();
    let first = NaiveDate::from_ymd_opt(today.year(), today.month(), 1).unwrap();
    let next = if today.month() == 12 { NaiveDate::from_ymd_opt(today.year() + 1, 1, 1) } else { NaiveDate::from_ymd_opt(today.year(), today.month() + 1, 1) }.unwrap();
    let (st, pl) = call(&env, Method::GET, &format!("/api/reports/profit?from={first}T00:00:00Z&to={next}T00:00:00Z&storeId={store2}"), Some(t), None).await;
    assert_eq!(st, StatusCode::OK, "{pl}");
    eq(&pl["sales"], 1000.0);
    eq(&pl["costOfSales"], 430.0);
    eq(&pl["grossIncome"], 570.0);

    // удаление возвращает материалы и деньги на счёт
    let (st, _) = call(&env, Method::DELETE, &format!("/api/complex-sales/{csid}"), Some(t), None).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(qty(&env, t, &store2, &milk).await, 10.0);
    let w: Value = call(&env, Method::GET, &format!("/api/wallets/{wallet}"), Some(t), None).await.1;
    eq(&w["balance"], 0.0);

    // ---- онлайн-каталог и заказы клиентов ----
    let (_, org) = call(&env, Method::GET, "/api/organizations", Some(t), None).await;
    let org_id = s(&org["id"]);
    let (st, _) = call(&env, Method::GET, &format!("/api/marketplace/products?organizationId={org_id}"), None, None).await;
    assert_eq!(st, StatusCode::NOT_FOUND); // каталог выключен
    let (st, _) = call(&env, Method::PATCH, "/api/marketplace-settings", Some(t), Some(json!({"storeForMarketplace": "bad"}))).await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
    let (st, mp) = call(&env, Method::PATCH, "/api/marketplace-settings", Some(t), Some(json!({
        "marketplaceEnabled": true, "storeForMarketplace": store, "description": "Лучшая студия"
    }))).await;
    assert_eq!(st, StatusCode::OK, "{mp}");
    let (_, orgs) = call(&env, Method::GET, "/api/marketplace/organizations", None, None).await;
    assert_eq!(orgs[0]["name"], "Studio");
    let (st, prods) = call(&env, Method::GET, &format!("/api/marketplace/products?organizationId={org_id}"), None, None).await;
    assert_eq!(st, StatusCode::OK);
    eq(&prods[0]["price"], 100.0);

    // публичный заказ: цену подставляет сервер, а не клиент
    let public = |phone: &str, qty: f64| json!({
        "organizationId": org_id, "clientName": "Пётр", "clientPhone": phone, "deliveryAddress": "Самовывоз",
        "items": [{"productId": milk, "qty": qty, "price": 1.0}]
    });
    let (st, _) = call(&env, Method::POST, "/api/orders/public", None, Some(public("89001234567", 2.0))).await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
    let (st, po) = call(&env, Method::POST, "/api/orders/public", None, Some(public("+79001234567", 2.0))).await;
    assert_eq!(st, StatusCode::OK, "{po}");
    eq(&po["total"], 200.0);
    let order_id = s(&po["id"]);

    let (_, cnt) = call(&env, Method::GET, "/api/orders/count/unprocessed", Some(t), None).await;
    assert_eq!(cnt["count"], 1);
    let (st, o) = call(&env, Method::PATCH, &format!("/api/orders/{order_id}/status"), Some(t), Some(json!({"status": "in_work"}))).await;
    assert_eq!(st, StatusCode::OK, "{o}");
    let (st, _) = call(&env, Method::PATCH, &format!("/api/orders/{order_id}/status"), Some(t), Some(json!({"status": "new"}))).await;
    assert_eq!(st, StatusCode::BAD_REQUEST);

    let before = qty(&env, t, &store, &milk).await;
    let (st, sale) = call(&env, Method::POST, &format!("/api/orders/{order_id}/create-sale"), Some(t), None).await;
    assert_eq!(st, StatusCode::OK, "{sale}");
    eq(&sale["total"], 200.0);
    assert_eq!(sale["orderId"], order_id.as_str());
    assert_eq!(qty(&env, t, &store, &milk).await, before - 2.0);
    let (_, o) = call(&env, Method::GET, &format!("/api/orders/{order_id}"), Some(t), None).await;
    assert_eq!(o["status"], "done");
    assert_eq!(o["saleId"], sale["id"]);
    let (st, _) = call(&env, Method::POST, &format!("/api/orders/{order_id}/create-sale"), Some(t), None).await;
    assert_eq!(st, StatusCode::BAD_REQUEST);

    // «скрывать товары без остатка»: заказ больше остатка отклоняется
    call(&env, Method::PATCH, "/api/marketplace-settings", Some(t), Some(json!({"disableNotInStockProducts": true}))).await;
    let (st, _) = call(&env, Method::POST, "/api/orders/public", None, Some(public("+79001234567", 100000.0))).await;
    assert_eq!(st, StatusCode::BAD_REQUEST);

    // ручной заказ с нестандартной позицией
    let (st, mo) = call(&env, Method::POST, "/api/orders", Some(t), Some(json!({
        "storeId": store, "clientName": "Ольга", "items": [{"name": "Подарочная упаковка", "qty": 1, "price": 50.0}]
    }))).await;
    assert_eq!(st, StatusCode::OK, "{mo}");
    eq(&mo["total"], 50.0);
    let (_, active) = call(&env, Method::GET, "/api/orders?activeOnly=true", Some(t), None).await;
    assert_eq!(active.as_array().unwrap().len(), 1);

    // права: роль без «Карточка клиента» видит маску телефона; продавец не меняет настройки
    let (st, role) = call(&env, Method::POST, "/api/roles", Some(t), Some(json!({"name": "Менеджер", "permissions": ["client.view"]}))).await;
    assert_eq!(st, StatusCode::OK, "{role}");
    call(&env, Method::POST, "/api/users", Some(t), Some(json!({
        "name": "M", "email": "mgr-cx@example.com", "password": "pass123", "roleId": s(&role["id"]), "storeIds": [store]
    }))).await;
    let (_, login) = call(&env, Method::POST, "/api/auth/login", None, Some(json!({"email": "mgr-cx@example.com", "password": "pass123"}))).await;
    let mt = s(&login["token"]);
    let (_, masked) = call(&env, Method::GET, "/api/clients", Some(&mt), None).await;
    let phone = s(&masked[0]["phone"]);
    assert!(phone.contains('•') && phone.ends_with("2233") && phone.starts_with("+7"), "{phone}");
    let (st, _) = call(&env, Method::GET, &format!("/api/clients/{cid}"), Some(&mt), None).await;
    assert_eq!(st, StatusCode::FORBIDDEN);
    let (st, _) = call(&env, Method::PATCH, "/api/loyalty-settings", Some(&mt), Some(json!({"cashbackEnabled": false}))).await;
    assert_eq!(st, StatusCode::FORBIDDEN);
}
