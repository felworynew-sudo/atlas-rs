//! Сквозной тест шага 8б: перемещения, производство (+ импорт), платежи, обновления, этикетки,
//! закрытие смены по способам оплаты и виды пунктов чек-листа.
use atlas_api::{app, AppState, MIGRATOR};
use axum::{
    body::Body,
    http::{header, Method, Request, StatusCode},
    Router,
};
use chrono::{Duration, Utc};
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
    rows.as_array().unwrap().iter().find(|r| r["productId"] == product).map_or(0.0, |r| r["qty"].as_f64().unwrap())
}

fn iso(d: chrono::DateTime<Utc>) -> String {
    d.format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

#[tokio::test]
async fn ops_flow() {
    let env = setup().await;
    let (st, body) = call(&env, Method::POST, "/api/auth/signup", None, Some(json!({
        "email": "ops@example.com", "password": "secret1", "name": "Owner", "organizationName": "Bakery", "storeName": "Пекарня"
    }))).await;
    assert_eq!(st, StatusCode::OK, "{body}");
    let t = s(&body["token"]);
    let t = t.as_str();
    let (_, org) = call(&env, Method::GET, "/api/organizations", Some(t), None).await;
    let mut modules = org["modules"].clone();
    for k in ["transfers", "manufacture", "payments", "updates", "label_printing", "shifts", "checklist"] {
        modules[k] = json!(true);
    }
    call(&env, Method::PUT, &format!("/api/organizations/{}", s(&org["id"])), Some(t), Some(json!({"modules": modules}))).await;

    let (_, stores) = call(&env, Method::GET, "/api/stores", Some(t), None).await;
    let a = s(&stores[0]["id"]);
    let (_, sb) = call(&env, Method::POST, "/api/stores", Some(t), Some(json!({"name": "Склад"}))).await;
    let b = s(&sb["id"]);
    let prices = |p: f64| json!([{"storeId": a, "salePrice": p}, {"storeId": b, "salePrice": p}]);
    let (_, milk) = call(&env, Method::POST, "/api/products", Some(t), Some(json!({"name": "Молоко", "purchasePrice": 50.0, "stores": prices(100.0)}))).await;
    let milk = s(&milk["id"]);
    let (_, flour) = call(&env, Method::POST, "/api/products", Some(t), Some(json!({"name": "Мука", "purchasePrice": 10.0, "allowFractional": true, "stores": prices(0.0)}))).await;
    let flour = s(&flour["id"]);
    let (st, bread) = call(&env, Method::POST, "/api/products", Some(t), Some(json!({
        "name": "Хлеб", "isComposite": true, "productionType": "ahead", "stores": prices(50.0),
        "ingredients": [{"productId": flour, "qty": 0.5}]
    }))).await;
    assert_eq!(st, StatusCode::OK, "{bread}");
    let bread = s(&bread["id"]);
    let (_, coffee) = call(&env, Method::POST, "/api/products", Some(t), Some(json!({
        "name": "Кофе", "isComposite": true, "productionType": "on_sale", "stores": prices(150.0), "ingredients": [{"productId": milk, "qty": 0.1}]
    }))).await;
    let coffee = s(&coffee["id"]);
    let (_, svc) = call(&env, Method::POST, "/api/products", Some(t), Some(json!({"name": "Доставка", "isService": true}))).await;
    let svc = s(&svc["id"]);
    call(&env, Method::PUT, &format!("/api/stocks/{milk}"), Some(t), Some(json!({"storeId": a, "qty": 10}))).await;
    call(&env, Method::PUT, &format!("/api/stocks/{flour}"), Some(t), Some(json!({"storeId": a, "qty": 100}))).await;

    // ---- перемещения ----
    let tr = |from: &str, to: &str, items: Value| json!({"fromStoreId": from, "toStoreId": to, "items": items});
    for (body, msg) in [
        (tr(&a, &a, json!([{"productId": milk, "qty": 1}])), "Филиалы должны отличаться"),
        (tr(&a, &b, json!([])), "Добавьте хотя бы один товар"),
        (tr(&a, &b, json!([{"productId": milk, "qty": 0}])), "Количество должно быть больше 0"),
        (tr(&a, &b, json!([{"productId": milk, "qty": 99}])), "Недостаточно товара «Молоко» в филиале (в наличии 10)"),
    ] {
        let (st, e) = call(&env, Method::POST, "/api/transfers", Some(t), Some(body)).await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "{e}");
        assert_eq!(e["message"], msg);
    }
    for pid in [&svc, &coffee] {
        let (st, _) = call(&env, Method::POST, "/api/transfers", Some(t), Some(tr(&a, &b, json!([{"productId": pid, "qty": 1}])))).await;
        assert_eq!(st, StatusCode::BAD_REQUEST); // услуги и «производимые при продаже» товары не перемещаются
    }
    let (st, tfr) = call(&env, Method::POST, "/api/transfers", Some(t), Some(tr(&a, &b, json!([{"productId": milk, "qty": 4}])))).await;
    assert_eq!(st, StatusCode::OK, "{tfr}");
    eq(&tfr["amount"], 200.0); // 4 × 50 по себестоимости
    assert_eq!(qty(&env, t, &a, &milk).await, 6.0);
    assert_eq!(qty(&env, t, &b, &milk).await, 4.0);
    let (_, by_b) = call(&env, Method::GET, &format!("/api/transfers?storeId={b}"), Some(t), None).await;
    assert_eq!(by_b.as_array().unwrap().len(), 1);
    let (st, _) = call(&env, Method::DELETE, &format!("/api/transfers/{}", s(&tfr["id"])), Some(t), None).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(qty(&env, t, &a, &milk).await, 10.0);
    assert_eq!(qty(&env, t, &b, &milk).await, 0.0);

    // ---- производство ----
    let (st, pv) = call(&env, Method::GET, &format!("/api/manufactures/preview?productId={bread}&qty=10"), Some(t), None).await;
    assert_eq!(st, StatusCode::OK, "{pv}");
    eq(&pv["ingredients"][0]["qty"], 5.0);
    eq(&pv["costTotal"], 50.0);
    let (st, _) = call(&env, Method::POST, "/api/manufactures", Some(t), Some(json!({"storeId": a, "productId": milk, "qty": 1}))).await;
    assert_eq!(st, StatusCode::BAD_REQUEST); // не составной
    let (st, _) = call(&env, Method::POST, "/api/manufactures", Some(t), Some(json!({"storeId": a, "productId": coffee, "qty": 1}))).await;
    assert_eq!(st, StatusCode::BAD_REQUEST); // производится при продаже
    let (st, m1) = call(&env, Method::POST, "/api/manufactures", Some(t), Some(json!({"storeId": a, "productId": bread, "qty": 10, "batch": "B-100"}))).await;
    assert_eq!(st, StatusCode::OK, "{m1}");
    eq(&m1["costTotal"], 50.0);
    assert_eq!(qty(&env, t, &a, &flour).await, 95.0);
    assert_eq!(qty(&env, t, &a, &bread).await, 10.0);
    let (_, m2) = call(&env, Method::POST, "/api/manufactures", Some(t), Some(json!({
        "storeId": a, "productId": bread, "qty": 10, "ingredients": [{"productId": flour, "qty": 6}]
    }))).await;
    eq(&m2["costTotal"], 60.0); // фактический расход муки 6 кг
    assert_eq!(qty(&env, t, &a, &flour).await, 89.0);
    let (st, _) = call(&env, Method::DELETE, &format!("/api/manufactures/{}", s(&m1["id"])), Some(t), None).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(qty(&env, t, &a, &flour).await, 94.0);
    assert_eq!(qty(&env, t, &a, &bread).await, 10.0);

    // импорт производства: товар + ингредиенты с общим «№ производства», пробный прогон и реальный
    let csv = "№ производства,дата,товар,количество,партия,ингредиент,количество ингредиента\n\
        1,2026-11-05,Хлеб,4,B-1,,\n\
        2,2026-11-05,Хлеб,2,B-2,,\n\
        2,,,,,Мука,1.5\n\
        3,,Неизвестный,1,,,\n";
    let (st, dry) = call(&env, Method::POST, "/api/import/manufactures", Some(t), Some(json!({"storeId": a, "content": csv, "dryRun": true}))).await;
    assert_eq!(st, StatusCode::OK, "{dry}");
    assert_eq!(dry["manufactures"], 2);
    assert!(s(&dry["errors"][0]["message"]).contains("не найден товар"), "{dry}");
    let (_, real) = call(&env, Method::POST, "/api/import/manufactures", Some(t), Some(json!({"storeId": a, "content": csv}))).await;
    assert_eq!(real["manufactures"], 2);
    assert_eq!(qty(&env, t, &a, &bread).await, 16.0); // 10 + 4 + 2
    assert_eq!(qty(&env, t, &a, &flour).await, 94.0 - 2.0 - 1.5);

    // ---- платежи ----
    let (_, w) = call(&env, Method::POST, "/api/wallets", Some(t), Some(json!({"name": "Банк"}))).await;
    let w = s(&w["id"]);
    call(&env, Method::POST, &format!("/api/wallets/{w}/transactions"), Some(t), Some(json!({"amount": 1000.0}))).await;
    let (_, cats) = call(&env, Method::GET, "/api/lists/spending_categories", Some(t), None).await;
    let rent = s(&cats[0]["id"]);
    let (st, e) = call(&env, Method::POST, "/api/payments", Some(t), Some(json!({
        "storeId": a, "walletId": w, "amount": 300.0, "periodFrom": "2026-10-31", "periodTo": "2026-10-01"
    }))).await;
    assert_eq!(st, StatusCode::BAD_REQUEST, "{e}");
    let (st, pay) = call(&env, Method::POST, "/api/payments", Some(t), Some(json!({
        "storeId": a, "walletId": w, "categoryId": rent, "amount": 300.0, "periodFrom": "2026-10-01", "periodTo": "2026-10-31", "details": "Аренда за октябрь"
    }))).await;
    assert_eq!(st, StatusCode::OK, "{pay}");
    let (_, wal) = call(&env, Method::GET, &format!("/api/wallets/{w}"), Some(t), None).await;
    eq(&wal["balance"], 700.0);
    let (_, inside) = call(&env, Method::GET, "/api/payments?by=period&from=2026-10-15&to=2026-10-20", Some(t), None).await;
    assert_eq!(inside.as_array().unwrap().len(), 1);
    let (_, outside) = call(&env, Method::GET, "/api/payments?by=period&from=2026-12-01&to=2026-12-31", Some(t), None).await;
    assert_eq!(outside.as_array().unwrap().len(), 0);
    let now = Utc::now();
    let (st, rep) = call(&env, Method::GET, &format!("/api/payments/report?from={}&to={}", iso(now - Duration::days(1)), iso(now + Duration::days(1))), Some(t), None).await;
    assert_eq!(st, StatusCode::OK, "{rep}");
    let row = rep["wallets"].as_array().unwrap().iter().find(|x| x["name"] == "Банк").unwrap();
    eq(&row["opening"], 0.0);
    eq(&row["inflow"], 1000.0);
    eq(&row["outflow"], 300.0);
    eq(&row["closing"], 700.0);
    eq(&row["byKind"]["payment"]["out"], 300.0);
    call(&env, Method::DELETE, &format!("/api/payments/{}", s(&pay["id"])), Some(t), None).await;
    let (_, wal) = call(&env, Method::GET, &format!("/api/wallets/{w}"), Some(t), None).await;
    eq(&wal["balance"], 1000.0);

    // ---- обновления ----
    let (_, cnt) = call(&env, Method::GET, "/api/updates/unread-count", Some(t), None).await;
    assert!(cnt["count"].as_i64().unwrap() >= 2); // удалены перемещение и производство
    let (st, ch) = call(&env, Method::PUT, &format!("/api/products/{milk}"), Some(t), Some(json!({
        "name": "Молоко", "purchasePrice": 50.0, "stores": [{"storeId": a, "salePrice": 120.0}, {"storeId": b, "salePrice": 100.0}]
    }))).await;
    assert_eq!(st, StatusCode::OK, "{ch}");
    let (_, price_events) = call(&env, Method::GET, "/api/updates?kind=price_changed", Some(t), None).await;
    assert_eq!(price_events.as_array().unwrap().len(), 1);
    assert_eq!(price_events[0]["text"], "Цена товара «Молоко» изменена 100 → 120 ₽");
    assert_eq!(price_events[0]["author"], "Owner");
    let (_, all) = call(&env, Method::GET, "/api/updates", Some(t), None).await;
    let kinds: Vec<&str> = all.as_array().unwrap().iter().map(|e| e["kind"].as_str().unwrap()).collect();
    assert!(kinds.contains(&"transfer_deleted") && kinds.contains(&"manufacture_deleted"), "{kinds:?}");
    assert_eq!(all[0]["unread"], true);
    call(&env, Method::POST, "/api/updates/read", Some(t), None).await;
    let (_, cnt) = call(&env, Method::GET, "/api/updates/unread-count", Some(t), None).await;
    assert_eq!(cnt["count"], 0);
    let (_, all) = call(&env, Method::GET, "/api/updates", Some(t), None).await;
    assert_eq!(all[0]["unread"], false);

    // ---- этикетки ----
    let (_, ls) = call(&env, Method::GET, "/api/label-settings", Some(t), None).await;
    eq(&ls["labelHeight"], 112.0);
    let (st, _) = call(&env, Method::PUT, "/api/label-settings", Some(t), Some(json!({"labelHeight": 5}))).await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
    let (st, _) = call(&env, Method::PUT, "/api/label-settings", Some(t), Some(json!({"nope": 1}))).await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
    call(&env, Method::PUT, "/api/label-settings", Some(t), Some(json!({"labelHeight": 150, "printDate": false}))).await;
    let render = json!({"storeId": a, "items": [{"productId": milk, "copies": 3}], "date": "2026-11-06"});
    let (st, lb) = call(&env, Method::POST, "/api/labels/render", Some(t), Some(render.clone())).await;
    assert_eq!(st, StatusCode::OK, "{lb}");
    eq(&lb["settings"]["labelHeight"], 150.0);
    eq(&lb["labels"][0]["price"], 120.0);
    assert_eq!(lb["labels"][0]["copies"], 3);
    assert!(lb["labels"][0]["date"].is_null());
    call(&env, Method::PUT, "/api/label-settings", Some(t), Some(json!({"printDate": true}))).await;
    let (_, lb) = call(&env, Method::POST, "/api/labels/render", Some(t), Some(render)).await;
    assert_eq!(lb["labels"][0]["date"], "2026-11-06");
    let (st, _) = call(&env, Method::POST, "/api/labels/render", Some(t), Some(json!({"storeId": a, "items": [{"productId": milk, "copies": 0}]}))).await;
    assert_eq!(st, StatusCode::BAD_REQUEST);

    // ---- смена: закрытие по способам оплаты ----
    let (_, pm) = call(&env, Method::GET, "/api/lists/payment_methods", Some(t), None).await;
    let (cash_pm, qr_pm) = (s(&pm[0]["id"]), s(&pm[1]["id"]));
    let (st, sh) = call(&env, Method::POST, "/api/shifts/open", Some(t), Some(json!({"storeId": a, "openingCash": 0.0}))).await;
    assert_eq!(st, StatusCode::OK, "{sh}");
    let sh = s(&sh["id"]);
    for pm_id in [&cash_pm, &qr_pm] {
        let (st, _) = call(&env, Method::POST, "/api/sales", Some(t), Some(json!({"storeId": a, "paymentMethodId": pm_id, "items": [{"productId": milk, "qty": 1}]}))).await;
        assert_eq!(st, StatusCode::OK);
    }
    let (_, cur) = call(&env, Method::GET, &format!("/api/shifts/{sh}"), Some(t), None).await;
    let qr_block = cur["totals"]["byMethod"].as_array().unwrap().iter().find(|m| m["name"] == "QR").unwrap();
    eq(&qr_block["sales"], 120.0);
    eq(&cur["totals"]["byUser"][0]["sales"], 240.0);
    assert_eq!(cur["totals"]["byUser"][0]["checks"], 2);
    let (st, _) = call(&env, Method::POST, &format!("/api/shifts/{sh}/close"), Some(t), Some(json!({
        "actualCash": 120.0, "actualByMethod": [{"paymentMethodId": cash_pm, "amount": 1.0}]
    }))).await;
    assert_eq!(st, StatusCode::BAD_REQUEST); // наличные закрываются отдельным полем
    let (st, closed) = call(&env, Method::POST, &format!("/api/shifts/{sh}/close"), Some(t), Some(json!({
        "actualCash": 120.0, "actualByMethod": [{"paymentMethodId": qr_pm, "amount": 110.0}]
    }))).await;
    assert_eq!(st, StatusCode::OK, "{closed}");
    eq(&closed["diff"], -10.0); // наличные сошлись, по QR не хватает 10
    eq(&closed["closeDetails"][0]["expected"], 120.0);
    eq(&closed["closeDetails"][0]["diff"], -10.0);

    // ---- виды пунктов чек-листа ----
    let (st, items) = call(&env, Method::PUT, "/api/checklist/items", Some(t), Some(json!({"storeId": a, "items": [
        {"title": "Открытие", "kind": "section"}, {"title": "Разовая задача", "kind": "once"}, {"title": "Ежедневная задача"}
    ]}))).await;
    assert_eq!(st, StatusCode::OK, "{items}");
    assert_eq!(items[0]["kind"], "section");
    assert_eq!(items[2]["kind"], "daily");
    let (st, _) = call(&env, Method::POST, &format!("/api/checklist/items/{}/toggle", s(&items[0]["id"])), Some(t), None).await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
    let (st, _) = call(&env, Method::PUT, "/api/checklist/items", Some(t), Some(json!({"storeId": a, "items": [{"title": "X", "kind": "weird"}]}))).await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
    for i in [1, 2] {
        call(&env, Method::POST, &format!("/api/checklist/items/{}/toggle", s(&items[i]["id"])), Some(t), None).await;
    }
    let (_, old_day) = call(&env, Method::GET, &format!("/api/checklist?storeId={a}&date=2000-01-01"), Some(t), None).await;
    assert_eq!(old_day[1]["done"], true); // разовая задача остаётся выполненной
    assert_eq!(old_day[2]["done"], false); // ежедневная привязана к дню

    // ---- права: продавцу недоступны перемещения и лента обновлений ----
    let (_, roles) = call(&env, Method::GET, "/api/roles", Some(t), None).await;
    let cashier = s(&roles.as_array().unwrap().iter().find(|r| r["key"] == "cashier").unwrap()["id"]);
    call(&env, Method::POST, "/api/users", Some(t), Some(json!({
        "name": "S", "email": "seller-ops@example.com", "password": "pass123", "roleId": cashier, "storeIds": [a]
    }))).await;
    let (_, login) = call(&env, Method::POST, "/api/auth/login", None, Some(json!({"email": "seller-ops@example.com", "password": "pass123"}))).await;
    let ct = s(&login["token"]);
    for uri in ["/api/transfers", "/api/updates", "/api/manufactures", "/api/payments"] {
        let (st, _) = call(&env, Method::GET, uri, Some(&ct), None).await;
        assert_eq!(st, StatusCode::FORBIDDEN, "{uri}");
    }
}
