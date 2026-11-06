//! Сквозной тест шага 7: импорт товаров и продаж из CSV (пробный прогон, ошибки по строкам, откат партии),
//! экспорт и массовое редактирование.
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

async fn raw(env: &Env, method: Method, uri: &str, token: Option<&str>, body: Option<Value>) -> (StatusCode, Vec<u8>) {
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
    (resp.status(), resp.into_body().collect().await.unwrap().to_bytes().to_vec())
}

async fn call(env: &Env, method: Method, uri: &str, token: Option<&str>, body: Option<Value>) -> (StatusCode, Value) {
    let (st, bytes) = raw(env, method, uri, token, body).await;
    (st, if bytes.is_empty() { Value::Null } else { serde_json::from_slice(&bytes).unwrap_or(Value::Null) })
}

fn s(v: &Value) -> String {
    v.as_str().unwrap_or_else(|| panic!("not a string: {v}")).to_string()
}

fn eq(v: &Value, x: f64) {
    let got = v.as_f64().unwrap_or_else(|| panic!("not a number: {v}"));
    assert!((got - x).abs() < 0.005, "expected {x}, got {got}");
}

async fn stock(env: &Env, t: &str, store: &str, product: &str) -> f64 {
    let (_, rows) = call(env, Method::GET, &format!("/api/stocks?storeId={store}"), Some(t), None).await;
    rows.as_array().unwrap().iter().find(|r| r["productId"] == product).unwrap()["qty"].as_f64().unwrap()
}

#[tokio::test]
async fn import_flow() {
    let env = setup().await;
    let (st, body) = call(&env, Method::POST, "/api/auth/signup", None, Some(json!({
        "email": "imp@example.com", "password": "secret1", "name": "Owner", "organizationName": "Shop", "storeName": "Точка"
    }))).await;
    assert_eq!(st, StatusCode::OK, "{body}");
    let t = s(&body["token"]);
    let t = t.as_str();
    let (_, stores) = call(&env, Method::GET, "/api/stores", Some(t), None).await;
    let store = s(&stores[0]["id"]);

    // ---- импорт товаров ----
    let products_csv = "название;цена;закупочная цена;штрихкод;категория;поставщик;остаток\n\
        Молоко;100,50;60;4600001;Напитки;Опт;20\n\
        Сыр;250;150;4600002;Еда;Опт;5\n\
        ;10;;;;;\n\
        Молоко;120;;;;;25\n\
        Хлеб;abc;;;;;\n";
    let req = |dry: bool| json!({"storeId": store, "content": products_csv, "dryRun": dry});
    let (st, dry) = call(&env, Method::POST, "/api/import/products", Some(t), Some(req(true))).await;
    assert_eq!(st, StatusCode::OK, "{dry}");
    assert_eq!(dry["dryRun"], true);
    assert_eq!(dry["created"], 2);
    assert_eq!(dry["updated"], 1);
    let errs = dry["errors"].as_array().unwrap();
    assert_eq!(errs.len(), 2);
    assert!(s(&errs[0]["message"]).starts_with("Строка 4:"), "{errs:?}");
    assert!(s(&errs[1]["message"]).starts_with("Строка 6:") && s(&errs[1]["message"]).contains("цена"), "{errs:?}");
    let (_, none) = call(&env, Method::GET, "/api/products", Some(t), None).await;
    assert_eq!(none.as_array().unwrap().len(), 0); // пробный прогон ничего не записал

    let (st, real) = call(&env, Method::POST, "/api/import/products", Some(t), Some(req(false))).await;
    assert_eq!(st, StatusCode::OK, "{real}");
    assert_eq!(real["created"], 2);
    let (_, prods) = call(&env, Method::GET, "/api/products", Some(t), None).await;
    assert_eq!(prods.as_array().unwrap().len(), 2);
    let milk = prods.as_array().unwrap().iter().find(|p| p["name"] == "Молоко").unwrap().clone();
    let cheese = prods.as_array().unwrap().iter().find(|p| p["name"] == "Сыр").unwrap().clone();
    let (milk_id, cheese_id) = (s(&milk["id"]), s(&cheese["id"]));
    eq(&milk["stores"][0]["salePrice"], 120.0); // вторая строка обновила цену
    eq(&milk["purchasePrice"], 60.0);
    assert_eq!(milk["barcodes"], json!(["4600001"]));
    assert_eq!(stock(&env, t, &store, &milk_id).await, 25.0);
    assert_eq!(stock(&env, t, &store, &cheese_id).await, 5.0);
    let (_, cats) = call(&env, Method::GET, "/api/product-categories", Some(t), None).await;
    assert_eq!(cats.as_array().unwrap().len(), 2);
    let (_, sups) = call(&env, Method::GET, "/api/suppliers", Some(t), None).await;
    assert_eq!(sups.as_array().unwrap().len(), 1);

    // повторный импорт обновляет, а не дублирует
    let (_, again) = call(&env, Method::POST, "/api/import/products", Some(t), Some(req(false))).await;
    assert_eq!(again["created"], 0);
    assert_eq!(again["updated"], 3);

    // некорректные файлы
    let (st, _) = call(&env, Method::POST, "/api/import/products", Some(t), Some(json!({"storeId": store, "content": ""}))).await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
    let (st, e) = call(&env, Method::POST, "/api/import/products", Some(t), Some(json!({"storeId": store, "content": "цена\n10"}))).await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
    assert_eq!(e["message"], "В файле нет колонки «название»");

    // ---- экспорт ----
    let (st, csv) = raw(&env, Method::GET, &format!("/api/export/products?storeId={store}"), Some(t), None).await;
    assert_eq!(st, StatusCode::OK);
    let csv = String::from_utf8(csv).unwrap();
    assert!(csv.contains("название,цена,закупочная цена,штрихкод,категория,поставщик,остаток"), "{csv}");
    assert!(csv.contains("Молоко,120.00,60.00,4600001,Напитки,Опт,25.00"), "{csv}");
    let (_, js) = call(&env, Method::GET, &format!("/api/export/products?storeId={store}&format=json"), Some(t), None).await;
    assert_eq!(js.as_array().unwrap().len(), 2);

    // ---- импорт продаж ----
    let sales_csv = "артикул,количество,клиент,дата\n\
        4600001,2,+79001112233,2026-11-05\n\
        Молоко,1,+79001112233,2026-11-05\n\
        4600002,1,,05.11.2026\n\
        NOPE,1,,2026-11-05\n\
        4600001,0,,2026-11-05\n";
    let sreq = |dry: bool| json!({"storeId": store, "content": sales_csv, "dryRun": dry});
    let (st, d) = call(&env, Method::POST, "/api/import/sales", Some(t), Some(sreq(true))).await;
    assert_eq!(st, StatusCode::OK, "{d}");
    assert_eq!(d["sales"], 2); // клиент+дата объединяют строки
    assert_eq!(d["items"], 3);
    eq(&d["total"], 610.0); // 3 × 120 + 250
    assert_eq!(d["errors"].as_array().unwrap().len(), 2);
    assert!(s(&d["errors"][0]["message"]).contains("не найден артикул"));
    assert_eq!(stock(&env, t, &store, &milk_id).await, 25.0);

    let (st, r) = call(&env, Method::POST, "/api/import/sales", Some(t), Some(sreq(false))).await;
    assert_eq!(st, StatusCode::OK, "{r}");
    assert_eq!(r["sales"], 2);
    let batch = s(&r["batchId"]);
    assert_eq!(stock(&env, t, &store, &milk_id).await, 22.0);
    assert_eq!(stock(&env, t, &store, &cheese_id).await, 4.0);
    let (_, list) = call(&env, Method::GET, &format!("/api/sales?storeId={store}&from=2026-11-05T00:00:00Z&to=2026-11-06T00:00:00Z"), Some(t), None).await;
    assert_eq!(list.as_array().unwrap().len(), 2);
    let (_, clients) = call(&env, Method::GET, "/api/clients", Some(t), None).await;
    assert_eq!(clients[0]["phone"], "+79001112233"); // клиент по телефону создан автоматически

    // откат партии возвращает остатки; повторный откат невозможен
    let (st, rv) = call(&env, Method::DELETE, &format!("/api/import/batches/{batch}"), Some(t), None).await;
    assert_eq!(st, StatusCode::OK, "{rv}");
    assert_eq!(rv["reverted"], 2);
    assert_eq!(stock(&env, t, &store, &milk_id).await, 25.0);
    assert_eq!(stock(&env, t, &store, &cheese_id).await, 5.0);
    let (st, _) = call(&env, Method::DELETE, &format!("/api/import/batches/{batch}"), Some(t), None).await;
    assert_eq!(st, StatusCode::NOT_FOUND);

    // ---- массовое редактирование ----
    let (st, ok) = call(&env, Method::PATCH, "/api/products/batch", Some(t), Some(json!({"items": [
        {"id": milk_id, "purchasePrice": 70.0, "categoryId": null, "stores": [{"storeId": store, "salePrice": 130.0}]},
        {"id": cheese_id, "name": "Сыр твёрдый", "hiddenInNewSale": true}
    ]}))).await;
    assert_eq!(st, StatusCode::OK, "{ok}");
    assert_eq!(ok["updated"], 2);
    let (_, m) = call(&env, Method::GET, &format!("/api/products/{milk_id}"), Some(t), None).await;
    eq(&m["purchasePrice"], 70.0);
    eq(&m["stores"][0]["salePrice"], 130.0);
    assert!(m["categoryId"].is_null());
    let (_, c) = call(&env, Method::GET, &format!("/api/products/{cheese_id}"), Some(t), None).await;
    assert_eq!(c["name"], "Сыр твёрдый");
    assert_eq!(c["hiddenInNewSale"], true);

    // всё или ничего: вторая запись неверна — первая не применяется
    let (st, e) = call(&env, Method::PATCH, "/api/products/batch", Some(t), Some(json!({"items": [
        {"id": milk_id, "name": "Молоко 3.2"}, {"id": cheese_id, "foo": 1}
    ]}))).await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
    assert_eq!(e["message"], "Строка 2: Поле «foo» нельзя менять массово");
    let (_, m) = call(&env, Method::GET, &format!("/api/products/{milk_id}"), Some(t), None).await;
    assert_eq!(m["name"], "Молоко");
    let (st, e) = call(&env, Method::PATCH, "/api/products/batch", Some(t), Some(json!({"items": [{"id": cheese_id, "barcodes": ["4600001"]}]}))).await;
    assert_eq!(st, StatusCode::CONFLICT, "{e}");
    assert_eq!(e["message"], "Строка 1: Штрихкод уже используется");
    let (st, _) = call(&env, Method::PATCH, "/api/products/batch", Some(t), Some(json!({"items": [{"id": milk_id}, {"id": milk_id}]}))).await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
    let (st, _) = call(&env, Method::PATCH, "/api/products/batch", Some(t), Some(json!({"items": []}))).await;
    assert_eq!(st, StatusCode::BAD_REQUEST);

    // права: продавцу импорт и экспорт недоступны
    let (_, roles) = call(&env, Method::GET, "/api/roles", Some(t), None).await;
    let cashier = s(&roles.as_array().unwrap().iter().find(|r| r["key"] == "cashier").unwrap()["id"]);
    call(&env, Method::POST, "/api/users", Some(t), Some(json!({
        "name": "S", "email": "seller-imp@example.com", "password": "pass123", "roleId": cashier, "storeIds": [store]
    }))).await;
    let (_, login) = call(&env, Method::POST, "/api/auth/login", None, Some(json!({"email": "seller-imp@example.com", "password": "pass123"}))).await;
    let ct = s(&login["token"]);
    let (st, _) = call(&env, Method::POST, "/api/import/products", Some(&ct), Some(req(true))).await;
    assert_eq!(st, StatusCode::FORBIDDEN);
    let (st, _) = call(&env, Method::GET, "/api/export/products", Some(&ct), None).await;
    assert_eq!(st, StatusCode::FORBIDDEN);
}
