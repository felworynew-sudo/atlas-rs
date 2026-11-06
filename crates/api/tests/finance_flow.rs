//! Сквозной тест финансов: счета, расходы, ежемесячные расходы, оплата приёмки, отчёты (продажи, P&L, товары, ревизии, CSV).
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

async fn raw(env: &Env, method: Method, uri: &str, token: Option<&str>, body: Option<Value>) -> (StatusCode, String, Vec<u8>) {
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
    let ct = resp.headers().get(header::CONTENT_TYPE).map(|v| v.to_str().unwrap().to_string()).unwrap_or_default();
    (status, ct, resp.into_body().collect().await.unwrap().to_bytes().to_vec())
}

async fn call(env: &Env, method: Method, uri: &str, token: Option<&str>, body: Option<Value>) -> (StatusCode, Value) {
    let (st, _, bytes) = raw(env, method, uri, token, body).await;
    (st, if bytes.is_empty() { Value::Null } else { serde_json::from_slice(&bytes).unwrap_or(Value::Null) })
}

fn s(v: &Value) -> String {
    v.as_str().unwrap().to_string()
}

fn eq(v: &Value, x: f64) {
    let got = v.as_f64().unwrap_or_else(|| panic!("not a number: {v}"));
    assert!((got - x).abs() < 0.005, "expected {x}, got {got}");
}

async fn balance(env: &Env, t: &str, wallet: &str) -> f64 {
    let (st, w) = call(env, Method::GET, &format!("/api/wallets/{wallet}"), Some(t), None).await;
    assert_eq!(st, StatusCode::OK);
    w["balance"].as_f64().unwrap()
}

#[tokio::test]
async fn finance_flow() {
    let env = setup().await;
    let (st, body) = call(&env, Method::POST, "/api/auth/signup", None, Some(json!({
        "email": "fin@example.com", "password": "secret1", "name": "Owner", "organizationName": "Shop", "storeName": "Точка"
    }))).await;
    assert_eq!(st, StatusCode::OK, "{body}");
    let t = s(&body["token"]);
    let t = t.as_str();
    let (_, stores) = call(&env, Method::GET, "/api/stores", Some(t), None).await;
    let store = s(&stores[0]["id"]);
    let (_, sup) = call(&env, Method::POST, "/api/suppliers", Some(t), Some(json!({"name": "Опт"}))).await;
    let sup = s(&sup["id"]);
    let (_, milk) = call(&env, Method::POST, "/api/products", Some(t), Some(json!({
        "name": "Молоко", "purchasePrice": 60.0, "supplierId": sup, "stores": [{"storeId": store, "salePrice": 100.0}]
    }))).await;
    let milk = s(&milk["id"]);

    // период — текущий месяц (UTC)
    let today = Utc::now().date_naive();
    let first = NaiveDate::from_ymd_opt(today.year(), today.month(), 1).unwrap();
    let next = if today.month() == 12 { NaiveDate::from_ymd_opt(today.year() + 1, 1, 1) } else { NaiveDate::from_ymd_opt(today.year(), today.month() + 1, 1) }.unwrap();
    let period = format!("from={first}T00:00:00Z&to={next}T00:00:00Z&storeId={store}");

    // денежные счета: пополнение, оплата приёмки, сторно при удалении
    let (st, cash) = call(&env, Method::POST, "/api/wallets", Some(t), Some(json!({"name": "Касса", "storeId": store}))).await;
    assert_eq!(st, StatusCode::OK, "{cash}");
    let cash = s(&cash["id"]);
    let (_, bank) = call(&env, Method::POST, "/api/wallets", Some(t), Some(json!({"name": "Банковский счёт"}))).await;
    let bank = s(&bank["id"]);
    let (st, _) = call(&env, Method::POST, &format!("/api/wallets/{cash}/transactions"), Some(t), Some(json!({"amount": 0}))).await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
    call(&env, Method::POST, &format!("/api/wallets/{cash}/transactions"), Some(t), Some(json!({"amount": 10000.0, "comment": "Внесение"}))).await;
    assert_eq!(balance(&env, t, &cash).await, 10000.0);

    let (st, sy1) = call(&env, Method::POST, "/api/supplies", Some(t), Some(json!({
        "storeId": store, "supplierId": sup, "paid": true, "walletId": cash, "items": [{"productId": milk, "qty": 20, "price": 70.0}]
    }))).await;
    assert_eq!(st, StatusCode::OK, "{sy1}");
    assert_eq!(balance(&env, t, &cash).await, 8600.0);
    let (st, _) = call(&env, Method::DELETE, &format!("/api/supplies/{}", s(&sy1["id"])), Some(t), None).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(balance(&env, t, &cash).await, 10000.0);
    // приёмка без оплаты — остаток 20, счёт не затронут
    let (st, _) = call(&env, Method::POST, "/api/supplies", Some(t), Some(json!({
        "storeId": store, "supplierId": sup, "items": [{"productId": milk, "qty": 20, "price": 70.0}]
    }))).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(balance(&env, t, &cash).await, 10000.0);

    // расходы
    let (_, cats) = call(&env, Method::GET, "/api/lists/spending_categories", Some(t), None).await;
    let rent = s(&cats[0]["id"]);
    let other = s(&cats[3]["id"]);
    for body in [
        json!({"storeId": store, "categoryId": rent, "amount": 0}),
        json!({"storeId": store, "amount": 100.0}),
        json!({"storeId": store, "categoryId": rent, "amount": 100.0, "kind": "weird"}),
    ] {
        let (st, e) = call(&env, Method::POST, "/api/spendings", Some(t), Some(body.clone())).await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "{body} -> {e}");
    }
    let (st, sp) = call(&env, Method::POST, "/api/spendings", Some(t), Some(json!({
        "storeId": store, "categoryId": rent, "amount": 500.0, "walletId": cash, "details": "Ноябрь"
    }))).await;
    assert_eq!(st, StatusCode::OK, "{sp}");
    assert_eq!(sp["categoryName"], "Аренда");
    assert_eq!(balance(&env, t, &cash).await, 9500.0);
    let (_, tmp) = call(&env, Method::POST, "/api/spendings", Some(t), Some(json!({"storeId": store, "categoryId": rent, "amount": 100.0, "walletId": cash}))).await;
    assert_eq!(balance(&env, t, &cash).await, 9400.0);
    let (st, _) = call(&env, Method::DELETE, &format!("/api/spendings/{}", s(&tmp["id"])), Some(t), None).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(balance(&env, t, &cash).await, 9500.0);
    let (st, _) = call(&env, Method::POST, "/api/spendings", Some(t), Some(json!({"storeId": store, "categoryId": other, "amount": 2000.0, "kind": "capital"}))).await;
    assert_eq!(st, StatusCode::OK);

    // перевод между счетами
    let (st, _) = call(&env, Method::POST, "/api/wallets/transfer", Some(t), Some(json!({"fromWalletId": cash, "toWalletId": cash, "amount": 1.0}))).await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
    let (st, _) = call(&env, Method::POST, "/api/wallets/transfer", Some(t), Some(json!({"fromWalletId": cash, "toWalletId": bank, "amount": 1000.0}))).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(balance(&env, t, &cash).await, 8500.0);
    assert_eq!(balance(&env, t, &bank).await, 1000.0);

    // ежемесячный расход: 30 000 с начала текущего месяца
    let (st, _) = call(&env, Method::POST, "/api/recurring-expenses", Some(t), Some(json!({
        "storeId": store, "name": "Аренда", "amount": 30000.0, "startDate": first.to_string(), "endDate": "2000-01-01"
    }))).await;
    assert_eq!(st, StatusCode::BAD_REQUEST); // конец раньше начала
    let (st, rec) = call(&env, Method::POST, "/api/recurring-expenses", Some(t), Some(json!({
        "storeId": store, "name": "Аренда", "amount": 30000.0, "startDate": first.to_string()
    }))).await;
    assert_eq!(st, StatusCode::OK, "{rec}");

    // комиссия терминала 2%
    let (_, pm) = call(&env, Method::GET, "/api/lists/payment_methods", Some(t), None).await;
    let cash_pm = s(&pm[0]["id"]);
    let term_pm = s(&pm[2]["id"]);
    let (st, _) = call(&env, Method::PUT, &format!("/api/lists/payment_methods/{term_pm}"), Some(t), Some(json!({
        "name": "Терминал", "params": {"kind": "terminal", "commission": 2.0}
    }))).await;
    assert_eq!(st, StatusCode::OK);

    // продажи: A — 2 шт наличными, B — 1 шт картой; возврат 1 шт из A на склад; списание 1; ревизия 16 → 15
    let (_, a) = call(&env, Method::POST, "/api/sales", Some(t), Some(json!({"storeId": store, "paymentMethodId": cash_pm, "items": [{"productId": milk, "qty": 2}]}))).await;
    let (st, _) = call(&env, Method::POST, "/api/sales", Some(t), Some(json!({"storeId": store, "paymentMethodId": term_pm, "items": [{"productId": milk, "qty": 1}]}))).await;
    assert_eq!(st, StatusCode::OK);
    let item = s(&a["items"][0]["id"]);
    let (st, r) = call(&env, Method::POST, &format!("/api/sales/{}/returns", s(&a["id"])), Some(t), Some(json!({
        "items": [{"saleItemId": item, "qty": 1}], "returnType": "money", "restock": true
    }))).await;
    assert_eq!(st, StatusCode::OK, "{r}");
    let (_, reasons) = call(&env, Method::GET, "/api/lists/writeoff_reasons", Some(t), None).await;
    call(&env, Method::POST, "/api/writeoffs", Some(t), Some(json!({"storeId": store, "productId": milk, "qty": 1, "reasonId": s(&reasons[0]["id"])}))).await;
    // остаток: 20 − 3 + 1 − 1 = 17 → факт 16 (минус 1 шт = −60 ₽)
    let (st, rev) = call(&env, Method::POST, "/api/revisions", Some(t), Some(json!({"storeId": store, "items": [{"productId": milk, "factQty": 16}]}))).await;
    assert_eq!(st, StatusCode::OK, "{rev}");
    eq(&rev["totalDiff"], -60.0);

    // отчёт по продажам
    let (st, rep) = call(&env, Method::GET, &format!("/api/reports/sales?{period}"), Some(t), None).await;
    assert_eq!(st, StatusCode::OK, "{rep}");
    eq(&rep["totals"]["sales"], 300.0);
    assert_eq!(rep["totals"]["count"], 2);
    eq(&rep["totals"]["avgCheck"], 150.0);
    eq(&rep["totals"]["returns"], 100.0);
    let groups = rep["groups"].as_array().unwrap();
    let g = |name: &str| groups.iter().find(|g| g["name"] == name).unwrap();
    eq(&g("Наличные")["sales"], 200.0);
    eq(&g("Наличные")["returns"], 100.0);
    eq(&g("Терминал")["sales"], 100.0);
    let (_, by_user) = call(&env, Method::GET, &format!("/api/reports/sales?{period}&groupBy=user"), Some(t), None).await;
    assert_eq!(by_user["groups"][0]["name"], "Owner");
    let (st, _) = call(&env, Method::GET, &format!("/api/reports/sales?{period}&groupBy=bogus"), Some(t), None).await;
    assert_eq!(st, StatusCode::BAD_REQUEST);

    // P&L: продажи 300, себестоимость 180 − 60 (возврат на склад) = 120, маржа 180;
    // −возвраты 100 −списания 60 −ревизия 60 −комиссия 2 (2% от 100) −расходы 500 −ежемесячные 30000
    let (st, pl) = call(&env, Method::GET, &format!("/api/reports/profit?{period}"), Some(t), None).await;
    assert_eq!(st, StatusCode::OK, "{pl}");
    eq(&pl["sales"], 300.0);
    eq(&pl["costOfSales"], 120.0);
    eq(&pl["grossIncome"], 180.0);
    eq(&pl["returns"], 100.0);
    eq(&pl["writeoffs"], 60.0);
    eq(&pl["revisionDiff"], -60.0);
    eq(&pl["bankCommission"], 2.0);
    eq(&pl["expenses"]["total"], 500.0);
    assert_eq!(pl["expenses"]["byCategory"][0]["name"], "Аренда");
    eq(&pl["monthlyExpenses"]["total"], 30000.0);
    eq(&pl["operatingProfit"], -30542.0);
    eq(&pl["capitalExpenses"], 2000.0);
    eq(&pl["netProfit"], -32542.0);
    eq(&pl["supplies"], 1400.0);

    // отчёт по товарам
    let (_, pr) = call(&env, Method::GET, &format!("/api/reports/products?{period}"), Some(t), None).await;
    let m = &pr["byProduct"][0];
    assert_eq!(m["name"], "Молоко");
    eq(&m["soldQty"], 2.0);
    eq(&m["soldSum"], 200.0);
    eq(&m["cost"], 120.0);
    eq(&m["profit"], 80.0);
    eq(&m["margin"], 40.0);
    eq(&m["writeoffQty"], 1.0);
    eq(&m["supplyQty"], 20.0);
    assert_eq!(pr["byCategory"][0]["name"], "Без категории");
    assert_eq!(pr["bySupplier"][0]["name"], "Опт");

    // отчёт по ревизиям
    let (_, rr) = call(&env, Method::GET, &format!("/api/reports/revisions?{period}"), Some(t), None).await;
    eq(&rr["minus"], -60.0);
    eq(&rr["plus"], 0.0);
    eq(&rr["balance"], -60.0);
    eq(&rr["minusList"][0]["was"], 17.0);
    eq(&rr["minusList"][0]["became"], 16.0);
    eq(&rr["minusList"][0]["diffAtSale"], -100.0);

    // экспорт CSV
    for name in ["sales", "profit", "products", "revisions"] {
        let (st, ct, bytes) = raw(&env, Method::GET, &format!("/api/reports/{name}?{period}&format=csv"), Some(t), None).await;
        assert_eq!(st, StatusCode::OK, "{name}");
        assert!(ct.starts_with("text/csv"), "{name}: {ct}");
        assert!(bytes.starts_with("\u{feff}".as_bytes()), "{name}: нет BOM");
    }
    let (_, _, csv) = raw(&env, Method::GET, &format!("/api/reports/profit?{period}&format=csv"), Some(t), None).await;
    let csv = String::from_utf8(csv).unwrap();
    assert!(csv.contains("Чистая прибыль,-32542.00"), "{csv}");

    // статистика главной (UTC-сутки): 2 чека на 300 ₽
    let (st, home) = call(&env, Method::GET, &format!("/api/reports/home?storeId={store}"), Some(t), None).await;
    assert_eq!(st, StatusCode::OK, "{home}");
    eq(&home["today"]["sales"], 300.0);
    assert_eq!(home["today"]["checks"], 2);
    eq(&home["weekAgo"]["sales"], 0.0);

    // права: продавец заносит расходы, но без права «Отчеты» и удаления расходов
    let (_, roles) = call(&env, Method::GET, "/api/roles", Some(t), None).await;
    let cashier = s(&roles.as_array().unwrap().iter().find(|r| r["key"] == "cashier").unwrap()["id"]);
    call(&env, Method::POST, "/api/users", Some(t), Some(json!({
        "name": "S", "email": "seller-fin@example.com", "password": "pass123", "roleId": cashier, "storeIds": [store]
    }))).await;
    let (_, login) = call(&env, Method::POST, "/api/auth/login", None, Some(json!({"email": "seller-fin@example.com", "password": "pass123"}))).await;
    let ct = s(&login["token"]);
    let ct = ct.as_str();
    // продавцу доступны отчёты («Отчеты»), но не статистика главной
    let (st, _) = call(&env, Method::GET, &format!("/api/reports/profit?{period}"), Some(ct), None).await;
    assert_eq!(st, StatusCode::OK);
    let (st, _) = call(&env, Method::GET, &format!("/api/reports/home?storeId={store}"), Some(ct), None).await;
    assert_eq!(st, StatusCode::FORBIDDEN);
    let (st, own) = call(&env, Method::POST, "/api/spendings", Some(ct), Some(json!({"storeId": store, "categoryId": rent, "amount": 50.0}))).await;
    assert_eq!(st, StatusCode::OK, "{own}");
    let (st, _) = call(&env, Method::DELETE, &format!("/api/spendings/{}", s(&own["id"])), Some(ct), None).await;
    assert_eq!(st, StatusCode::FORBIDDEN);
    let (st, _) = call(&env, Method::POST, "/api/wallets", Some(ct), Some(json!({"name": "X"}))).await;
    assert_eq!(st, StatusCode::FORBIDDEN);
}
