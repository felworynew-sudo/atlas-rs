//! Сквозной тест шага 8а: смена и касса, расчёт наличных, недосдача, автозакрытие, зарплата, рабочие часы, чеклист.
use atlas_api::{app, AppState, MIGRATOR};
use axum::{
    body::Body,
    http::{header, Method, Request, StatusCode},
    Router,
};
use chrono::{Duration, Utc};
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

fn iso(d: chrono::DateTime<Utc>) -> String {
    d.format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

#[tokio::test]
async fn shift_flow() {
    let env = setup().await;
    let (st, body) = call(&env, Method::POST, "/api/auth/signup", None, Some(json!({
        "email": "shift@example.com", "password": "secret1", "name": "Owner", "organizationName": "Cafe", "storeName": "Точка"
    }))).await;
    assert_eq!(st, StatusCode::OK, "{body}");
    let t = s(&body["token"]);
    let t = t.as_str();
    let (_, org) = call(&env, Method::GET, "/api/organizations", Some(t), None).await;
    let mut modules = org["modules"].clone();
    for k in ["shifts", "salary", "checklist", "payments"] {
        modules[k] = json!(true);
    }
    call(&env, Method::PUT, &format!("/api/organizations/{}", s(&org["id"])), Some(t), Some(json!({"modules": modules}))).await;

    let (_, stores) = call(&env, Method::GET, "/api/stores", Some(t), None).await;
    let store = s(&stores[0]["id"]);
    let (_, milk) = call(&env, Method::POST, "/api/products", Some(t), Some(json!({
        "name": "Молоко", "purchasePrice": 60.0, "stores": [{"storeId": store, "salePrice": 100.0}]
    }))).await;
    let milk = s(&milk["id"]);
    call(&env, Method::PUT, &format!("/api/stocks/{milk}"), Some(t), Some(json!({"storeId": store, "qty": 50}))).await;
    let (_, pm) = call(&env, Method::GET, "/api/lists/payment_methods", Some(t), None).await;
    let (cash_pm, qr_pm) = (s(&pm[0]["id"]), s(&pm[1]["id"]));

    // касса: счёт «Наличные с кассы» с начальным остатком 1000
    let (st, wallet) = call(&env, Method::POST, "/api/wallets", Some(t), Some(json!({"name": "Касса", "storeId": store, "isCashRegister": true}))).await;
    assert_eq!(st, StatusCode::OK, "{wallet}");
    assert_eq!(wallet["isCashRegister"], true);
    let wallet = s(&wallet["id"]);
    call(&env, Method::POST, &format!("/api/wallets/{wallet}/transactions"), Some(t), Some(json!({"amount": 1000.0}))).await;
    let (_, cats) = call(&env, Method::GET, "/api/lists/spending_categories", Some(t), None).await;
    let rent = s(&cats[0]["id"]);

    // ставка роли «Продавец»: 300 ₽/час + 5% от личных продаж
    let (_, roles) = call(&env, Method::GET, "/api/roles", Some(t), None).await;
    let cashier_role = s(&roles.as_array().unwrap().iter().find(|r| r["key"] == "cashier").unwrap()["id"]);
    let (st, r) = call(&env, Method::PUT, &format!("/api/roles/{cashier_role}"), Some(t), Some(json!({
        "name": "Продавец", "salaryType": "hour", "salaryRate": 300.0, "bonusType": "percent_own_sales", "bonusRate": 5.0
    }))).await;
    assert_eq!(st, StatusCode::OK, "{r}");
    eq(&r["bonusRate"], 5.0);
    let (_, u) = call(&env, Method::POST, "/api/users", Some(t), Some(json!({
        "name": "Катя", "email": "kate@example.com", "password": "pass123", "roleId": cashier_role, "storeIds": [store]
    }))).await;
    let kate = s(&u["id"]);
    let (_, login) = call(&env, Method::POST, "/api/auth/login", None, Some(json!({"email": "kate@example.com", "password": "pass123"}))).await;
    let ct = s(&login["token"]);
    let ct = ct.as_str();

    // без открытой смены продавец продавать не может
    let sale = |pm: &str, qty: u32| json!({"storeId": store, "paymentMethodId": pm, "items": [{"productId": milk, "qty": qty}]});
    let (st, e) = call(&env, Method::POST, "/api/sales", Some(ct), Some(sale(&cash_pm, 1))).await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
    assert_eq!(e["message"], "Откройте смену, чтобы оформить продажу");

    // открытие: остаток по умолчанию — баланс кассы; вторая открытая смена невозможна
    let (st, none) = call(&env, Method::GET, &format!("/api/shifts/current?storeId={store}"), Some(ct), None).await;
    assert_eq!(st, StatusCode::OK);
    assert!(none.is_null());
    let (st, shift) = call(&env, Method::POST, "/api/shifts/open", Some(ct), Some(json!({"storeId": store}))).await;
    assert_eq!(st, StatusCode::OK, "{shift}");
    eq(&shift["openingCash"], 1000.0);
    let shift_id = s(&shift["id"]);
    assert_eq!(shift["staff"].as_array().unwrap().len(), 1);
    let (st, _) = call(&env, Method::POST, "/api/shifts/open", Some(t), Some(json!({"storeId": store}))).await;
    assert_eq!(st, StatusCode::CONFLICT);

    // продажи: 2 шт наличными, 1 шт по QR; возврат 1 шт из наличной; расход, изъятие и внесение
    let (_, s1) = call(&env, Method::POST, "/api/sales", Some(ct), Some(sale(&cash_pm, 2))).await;
    eq(&s1["total"], 200.0);
    let (st, _) = call(&env, Method::POST, "/api/sales", Some(ct), Some(sale(&qr_pm, 1))).await;
    assert_eq!(st, StatusCode::OK);
    let item = s(&s1["items"][0]["id"]);
    let (st, _) = call(&env, Method::POST, &format!("/api/sales/{}/returns", s(&s1["id"])), Some(ct), Some(json!({
        "items": [{"saleItemId": item, "qty": 1}], "returnType": "money", "restock": true
    }))).await;
    assert_eq!(st, StatusCode::OK);
    let (st, _) = call(&env, Method::POST, "/api/spendings", Some(t), Some(json!({"storeId": store, "categoryId": rent, "amount": 50.0, "walletId": wallet}))).await;
    assert_eq!(st, StatusCode::OK);
    let (st, _) = call(&env, Method::POST, &format!("/api/shifts/{shift_id}/cashbox"), Some(ct), Some(json!({"amount": -300.0, "comment": "Инкассация"}))).await;
    assert_eq!(st, StatusCode::OK);
    let (st, _) = call(&env, Method::POST, &format!("/api/shifts/{shift_id}/cashbox"), Some(ct), Some(json!({"amount": 20.0}))).await;
    assert_eq!(st, StatusCode::OK);
    let (st, _) = call(&env, Method::POST, &format!("/api/shifts/{shift_id}/cashbox"), Some(ct), Some(json!({"amount": 0}))).await;
    assert_eq!(st, StatusCode::BAD_REQUEST);

    let (_, cur) = call(&env, Method::GET, &format!("/api/shifts/current?storeId={store}"), Some(ct), None).await;
    let tt = &cur["totals"];
    eq(&tt["sales"], 300.0);
    assert_eq!(tt["checks"], 2);
    eq(&tt["cashSales"], 200.0);
    eq(&tt["nonCashSales"], 100.0);
    eq(&tt["cashReturns"], 100.0);
    eq(&tt["cashSpendings"], 50.0);
    eq(&tt["deposits"], 20.0);
    eq(&tt["withdrawals"], 300.0);
    eq(&tt["expectedCash"], 770.0); // 1000 + 200 − 100 − 300 − 50 + 20

    // сотрудники смены: добавить может только тот, у кого есть право
    let (st, _) = call(&env, Method::POST, &format!("/api/shifts/{shift_id}/staff"), Some(ct), Some(json!({"userId": kate}))).await;
    assert_eq!(st, StatusCode::FORBIDDEN);
    let (_, me) = call(&env, Method::GET, "/api/auth/me", Some(t), None).await;
    let (st, v) = call(&env, Method::POST, &format!("/api/shifts/{shift_id}/staff"), Some(t), Some(json!({"userId": me["id"]}))).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(v["staff"].as_array().unwrap().len(), 2);

    // закрытие: недосдача 10 ₽; повторно закрыть и вносить в закрытую смену нельзя; отмена закрытия
    let (st, _) = call(&env, Method::POST, &format!("/api/shifts/{shift_id}/close"), Some(ct), Some(json!({"actualCash": -5}))).await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
    let (st, closed) = call(&env, Method::POST, &format!("/api/shifts/{shift_id}/close"), Some(ct), Some(json!({"actualCash": 760.0, "comment": "Не хватило сдачи"}))).await;
    assert_eq!(st, StatusCode::OK, "{closed}");
    eq(&closed["expectedCash"], 770.0);
    eq(&closed["diff"], -10.0);
    assert!(!closed["closedAt"].is_null());
    let (st, _) = call(&env, Method::POST, &format!("/api/shifts/{shift_id}/close"), Some(ct), Some(json!({"actualCash": 760.0}))).await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
    let (st, _) = call(&env, Method::POST, &format!("/api/shifts/{shift_id}/cashbox"), Some(ct), Some(json!({"amount": 5.0}))).await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
    let (st, reopened) = call(&env, Method::POST, &format!("/api/shifts/{shift_id}/reopen"), Some(t), None).await;
    assert_eq!(st, StatusCode::OK, "{reopened}");
    assert!(reopened["closedAt"].is_null());
    let (st, _) = call(&env, Method::POST, &format!("/api/shifts/{shift_id}/close"), Some(ct), Some(json!({"actualCash": 760.0}))).await;
    assert_eq!(st, StatusCode::OK);

    // зарплата: 2 ч вручную × 300 + бонус 5% от чистых продаж 200 ₽ = 610 (плюс секунды самой смены)
    let (now, from) = (Utc::now(), Utc::now() - Duration::days(2));
    let to = now + Duration::days(1);
    let period = format!("from={}&to={}", iso(from), iso(to));
    let (st, wh) = call(&env, Method::POST, "/api/working-hours", Some(t), Some(json!({
        "userId": kate, "storeId": store, "startedAt": iso(now - Duration::hours(3)), "endedAt": iso(now - Duration::hours(1))
    }))).await;
    assert_eq!(st, StatusCode::OK, "{wh}");
    let (st, _) = call(&env, Method::POST, "/api/working-hours", Some(t), Some(json!({
        "userId": kate, "storeId": store, "startedAt": iso(now), "endedAt": iso(now - Duration::hours(1))
    }))).await;
    assert_eq!(st, StatusCode::BAD_REQUEST); // окончание раньше начала
    let (st, _) = call(&env, Method::GET, &format!("/api/salary?{period}"), Some(ct), None).await;
    assert_eq!(st, StatusCode::FORBIDDEN); // у продавца нет права «Зарплата»

    let (st, list) = call(&env, Method::GET, &format!("/api/salary?{period}"), Some(t), None).await;
    assert_eq!(st, StatusCode::OK, "{list}");
    let row = list.as_array().unwrap().iter().find(|r| r["name"] == "Катя").unwrap();
    assert_eq!(row["shifts"], 2);
    let base = row["base"].as_f64().unwrap();
    assert!((600.0..601.0).contains(&base), "{base}");
    eq(&row["bonus"], 10.0);
    let accrued = row["accrued"].as_f64().unwrap();
    eq(&row["paid"], 0.0);

    // выплата со счёта кассы и её отмена
    let (_, before) = call(&env, Method::GET, &format!("/api/wallets/{wallet}"), Some(t), None).await;
    let (st, pay) = call(&env, Method::POST, &format!("/api/salary/{kate}/payments"), Some(t), Some(json!({"amount": 100.0, "walletId": wallet}))).await;
    assert_eq!(st, StatusCode::OK, "{pay}");
    let (_, after) = call(&env, Method::GET, &format!("/api/wallets/{wallet}"), Some(t), None).await;
    eq(&after["balance"], before["balance"].as_f64().unwrap() - 100.0);
    let (_, det) = call(&env, Method::GET, &format!("/api/salary/{kate}?{period}"), Some(t), None).await;
    eq(&det["paid"], 100.0);
    eq(&det["toPay"], accrued - 100.0);
    assert_eq!(det["workingHours"].as_array().unwrap().len(), 2);
    assert_eq!(det["payments"].as_array().unwrap().len(), 1);
    // за всё время учитываются только часы после регистрации сотрудника: бонус 10 + секунды смены − выплата 100
    let all = det["balanceAll"].as_f64().unwrap();
    assert!(all < 0.0 && all > -100.0, "{all}");
    let (st, _) = call(&env, Method::DELETE, &format!("/api/salary-payments/{}", s(&pay["id"])), Some(t), None).await;
    assert_eq!(st, StatusCode::OK);
    let (_, back) = call(&env, Method::GET, &format!("/api/wallets/{wallet}"), Some(t), None).await;
    eq(&back["balance"], before["balance"].as_f64().unwrap());
    let (st, _) = call(&env, Method::POST, &format!("/api/salary/{kate}/payments"), Some(ct), Some(json!({"amount": 1.0}))).await;
    assert_eq!(st, StatusCode::FORBIDDEN);

    // P&L: зарплаты и разница закрытия смен
    let (st, pl) = call(&env, Method::GET, &format!("/api/reports/profit?{period}"), Some(t), None).await;
    assert_eq!(st, StatusCode::OK, "{pl}");
    eq(&pl["shiftDiff"], -10.0);
    eq(&pl["salaries"], accrued);

    // автозакрытие: до времени закрытия ничего, через двое суток — смена закрыта автоматически
    let (st, sh2) = call(&env, Method::POST, "/api/shifts/open", Some(ct), Some(json!({"storeId": store, "openingCash": 500.0}))).await;
    assert_eq!(st, StatusCode::OK, "{sh2}");
    let sh2 = s(&sh2["id"]);
    assert_eq!(atlas_api::routes::shifts::auto_close_due(&env.pool, Utc::now()).await.unwrap(), 0);
    assert_eq!(atlas_api::routes::shifts::auto_close_due(&env.pool, Utc::now() + Duration::days(2)).await.unwrap(), 1);
    let (_, closed2) = call(&env, Method::GET, &format!("/api/shifts/{sh2}"), Some(ct), None).await;
    assert_eq!(closed2["autoClosed"], true);
    assert_eq!(closed2["comment"], "Автозакрытие");
    eq(&closed2["diff"], 0.0);
    let (_, hist) = call(&env, Method::GET, &format!("/api/shifts?storeId={store}"), Some(ct), None).await;
    assert_eq!(hist.as_array().unwrap().len(), 2);

    // ---- чеклист ----
    let (st, items) = call(&env, Method::PUT, "/api/checklist/items", Some(t), Some(json!({"storeId": store, "items": [
        {"title": "Открыть окно", "time": "09:00"}, {"title": "Проверить кассу"}
    ]}))).await;
    assert_eq!(st, StatusCode::OK, "{items}");
    assert_eq!(items.as_array().unwrap().len(), 2);
    let first = s(&items[0]["id"]);
    let (st, _) = call(&env, Method::PUT, "/api/checklist/items", Some(t), Some(json!({"storeId": store, "items": [{"title": "X", "time": "25:00"}]}))).await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
    let (st, _) = call(&env, Method::PUT, "/api/checklist/items", Some(ct), Some(json!({"storeId": store, "items": []}))).await;
    assert_eq!(st, StatusCode::FORBIDDEN);
    let (_, day) = call(&env, Method::GET, &format!("/api/checklist?storeId={store}"), Some(ct), None).await;
    assert_eq!(day[0]["done"], false);
    let (st, done) = call(&env, Method::POST, &format!("/api/checklist/items/{first}/toggle"), Some(ct), None).await;
    assert_eq!(st, StatusCode::OK, "{done}");
    assert_eq!(done["done"], true);
    assert_eq!(done["doneBy"], "Катя");
    let (_, other_day) = call(&env, Method::GET, &format!("/api/checklist?storeId={store}&date=2000-01-01"), Some(ct), None).await;
    assert_eq!(other_day[0]["done"], false); // отметки привязаны к дню
    let (_, undone) = call(&env, Method::POST, &format!("/api/checklist/items/{first}/toggle"), Some(ct), None).await;
    assert_eq!(undone["done"], false);
    let (_, kept) = call(&env, Method::PUT, "/api/checklist/items", Some(t), Some(json!({"storeId": store, "items": [{"id": first, "title": "Открыть окно!", "time": "09:30"}]}))).await;
    assert_eq!(kept.as_array().unwrap().len(), 1);
    assert_eq!(kept[0]["title"], "Открыть окно!");
}
