//! Сквозной тест каталога: единицы по умолчанию, категории, поставщики, товары, составные, копия, права.
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

#[tokio::test]
async fn catalog_flow() {
    let env = setup().await;
    let email = format!("catalog-{}@example.com", uuid_suffix());
    let (st, body) = call(&env, Method::POST, "/api/auth/signup", None, Some(json!({
        "email": email, "password": "secret1", "name": "Owner", "organizationName": "Cafe", "storeName": "Зал 1"
    }))).await;
    assert_eq!(st, StatusCode::OK, "{body}");
    let t = body["token"].as_str().unwrap().to_string();
    let t = t.as_str();

    // единицы по умолчанию
    let (st, units) = call(&env, Method::GET, "/api/product-units", Some(t), None).await;
    assert_eq!(st, StatusCode::OK);
    let names: Vec<&str> = units.as_array().unwrap().iter().map(|u| u["name"].as_str().unwrap()).collect();
    assert_eq!(names, vec!["Штука", "Килограмм", "Грамм", "Литр", "Миллилитр"]);
    let piece = units[0]["id"].as_str().unwrap().to_string();

    // новая единица и защита от удаления используемой
    let (st, u) = call(&env, Method::POST, "/api/product-units", Some(t), Some(json!({"name": "Порция", "shortName": "порц"}))).await;
    assert_eq!(st, StatusCode::OK, "{u}");
    let (st, _) = call(&env, Method::POST, "/api/product-units", Some(t), Some(json!({"name": "  "}))).await;
    assert_eq!(st, StatusCode::BAD_REQUEST);

    // категории и поставщики
    let (st, cat) = call(&env, Method::POST, "/api/product-categories", Some(t), Some(json!({"name": "Напитки"}))).await;
    assert_eq!(st, StatusCode::OK, "{cat}");
    let cat_id = cat["id"].as_str().unwrap().to_string();
    let (_, cat2) = call(&env, Method::POST, "/api/product-categories", Some(t), Some(json!({"name": "Еда"}))).await;
    assert_eq!(cat2["sortOrder"].as_i64().unwrap(), cat["sortOrder"].as_i64().unwrap() + 1);
    let (st, sup) = call(&env, Method::POST, "/api/suppliers", Some(t), Some(json!({"name": "Молочный двор"}))).await;
    assert_eq!(st, StatusCode::OK, "{sup}");
    let sup_id = sup["id"].as_str().unwrap().to_string();

    // филиалы организации
    let (_, stores) = call(&env, Method::GET, "/api/stores", Some(t), None).await;
    let store1 = stores[0]["id"].as_str().unwrap().to_string();
    let (_, s2) = call(&env, Method::POST, "/api/stores", Some(t), Some(json!({"name": "Зал 2"}))).await;
    let store2 = s2["id"].as_str().unwrap().to_string();

    // товары: молоко и сироп (обычные)
    let (st, milk) = call(&env, Method::POST, "/api/products", Some(t), Some(json!({
        "name": "Молоко 1л", "purchasePrice": 100.0, "extraCost": 5.0, "categoryId": cat_id, "supplierId": sup_id,
        "unitId": piece, "barcodes": [" 4600001 ", "4600001", "", "4600002"],
        "stores": [{"storeId": store1, "salePrice": 150.0}, {"storeId": store2, "salePrice": 160.0, "active": false}]
    }))).await;
    assert_eq!(st, StatusCode::OK, "{milk}");
    let milk_id = milk["id"].as_str().unwrap().to_string();
    assert_eq!(milk["barcodes"], json!(["4600001", "4600002"]));
    assert_eq!(milk["costPrice"], 105.0);
    assert_eq!(milk["stores"].as_array().unwrap().len(), 2);

    // повторный штрихкод — 409; пустое имя — 400; чужая/несуществующая категория — 400
    let (st, e) = call(&env, Method::POST, "/api/products", Some(t), Some(json!({"name": "Дубль", "barcodes": ["4600002"]}))).await;
    assert_eq!(st, StatusCode::CONFLICT, "{e}");
    assert_eq!(e["message"], "Штрихкод уже используется");
    let (st, _) = call(&env, Method::POST, "/api/products", Some(t), Some(json!({"name": " "}))).await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
    let (st, _) = call(&env, Method::POST, "/api/products", Some(t), Some(json!({"name": "X", "categoryId": "00000000-0000-0000-0000-000000000001"}))).await;
    assert_eq!(st, StatusCode::BAD_REQUEST);

    let (st, syrup) = call(&env, Method::POST, "/api/products", Some(t), Some(json!({
        "name": "Сироп", "purchasePrice": 40.0, "hiddenInNewSale": true, "allowFractional": true,
        "stores": [{"storeId": store1, "salePrice": 0.0}]
    }))).await;
    assert_eq!(st, StatusCode::OK, "{syrup}");
    let syrup_id = syrup["id"].as_str().unwrap().to_string();

    // составной товар: себестоимость из состава
    let (st, latte) = call(&env, Method::POST, "/api/products", Some(t), Some(json!({
        "name": "Латте", "isComposite": true, "productionType": "on_sale", "extraCost": 10.0, "purchasePrice": 999.0,
        "categoryId": cat_id,
        "stores": [{"storeId": store1, "salePrice": 250.0}],
        "ingredients": [{"productId": milk_id, "qty": 0.2}, {"productId": syrup_id, "qty": 0.5}]
    }))).await;
    assert_eq!(st, StatusCode::OK, "{latte}");
    let latte_id = latte["id"].as_str().unwrap().to_string();
    assert_eq!(latte["costPrice"], 51.0); // 0.2*105 + 0.5*40 + 10
    assert_eq!(latte["ingredients"].as_array().unwrap().len(), 2);

    // валидации состава
    let (st, _) = call(&env, Method::POST, "/api/products", Some(t), Some(json!({"name": "Услуга", "isService": true, "isComposite": true}))).await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
    let (st, _) = call(&env, Method::POST, "/api/products", Some(t), Some(json!({"name": "Не составной", "ingredients": [{"productId": milk_id, "qty": 1.0}]}))).await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
    let (st, _) = call(&env, Method::POST, "/api/products", Some(t), Some(json!({"name": "Ноль", "isComposite": true, "ingredients": [{"productId": milk_id, "qty": 0.0}]}))).await;
    assert_eq!(st, StatusCode::BAD_REQUEST);

    // цикл: молоко становится составным и включает латте
    let (st, e) = call(&env, Method::PUT, &format!("/api/products/{milk_id}"), Some(t), Some(json!({
        "name": "Молоко 1л", "isComposite": true, "ingredients": [{"productId": latte_id, "qty": 1.0}]
    }))).await;
    assert_eq!(st, StatusCode::BAD_REQUEST, "{e}");
    assert_eq!(e["message"], "Ингредиент создаёт цикл в составе");

    // обновление: пересчёт себестоимости латте после смены цены молока
    let (st, upd) = call(&env, Method::PUT, &format!("/api/products/{milk_id}"), Some(t), Some(json!({
        "name": "Молоко 1л", "purchasePrice": 200.0, "extraCost": 0.0, "barcodes": ["4600001"],
        "categoryId": cat_id, "supplierId": sup_id, "unitId": piece,
        "stores": [{"storeId": store1, "salePrice": 150.0}, {"storeId": store2, "salePrice": 160.0, "active": false}]
    }))).await;
    assert_eq!(st, StatusCode::OK, "{upd}");
    let (_, latte) = call(&env, Method::GET, &format!("/api/products/{latte_id}"), Some(t), None).await;
    assert_eq!(latte["costPrice"], 70.0); // 0.2*200 + 0.5*40 + 10

    // список: поиск по имени и штрихкоду, фильтр категории, скрытие неактивных в филиале
    let (_, l) = call(&env, Method::GET, "/api/products?search=%D0%BB%D0%B0%D1%82", Some(t), None).await; // «лат»
    assert_eq!(l.as_array().unwrap().len(), 1);
    let (_, l) = call(&env, Method::GET, "/api/products?search=4600001", Some(t), None).await;
    assert_eq!(l.as_array().unwrap().len(), 1);
    let (_, l) = call(&env, Method::GET, &format!("/api/products?categoryId={cat_id}"), Some(t), None).await;
    assert_eq!(l.as_array().unwrap().len(), 2); // молоко + латте
    let (_, l) = call(&env, Method::GET, &format!("/api/products?storeId={store2}"), Some(t), None).await;
    assert_eq!(l.as_array().unwrap().len(), 2); // молоко неактивно в зале 2 — но админ без includeInactive его не видит
    let (_, l) = call(&env, Method::GET, &format!("/api/products?storeId={store2}&includeInactive=true"), Some(t), None).await;
    assert_eq!(l.as_array().unwrap().len(), 3);

    // копия: без штрихкодов, с составом
    let (st, cp) = call(&env, Method::POST, &format!("/api/products/{latte_id}/copy"), Some(t), None).await;
    assert_eq!(st, StatusCode::OK, "{cp}");
    assert_eq!(cp["name"], "Латте (копия)");
    assert_eq!(cp["ingredients"].as_array().unwrap().len(), 2);

    // удаление: ингредиент используется — нельзя; после удаления составных — можно
    let (st, e) = call(&env, Method::DELETE, &format!("/api/products/{syrup_id}"), Some(t), None).await;
    assert_eq!(st, StatusCode::BAD_REQUEST, "{e}");
    for id in [latte_id.as_str(), cp["id"].as_str().unwrap()] {
        let (st, _) = call(&env, Method::DELETE, &format!("/api/products/{id}"), Some(t), None).await;
        assert_eq!(st, StatusCode::OK);
    }
    let (st, _) = call(&env, Method::DELETE, &format!("/api/products/{syrup_id}"), Some(t), None).await;
    assert_eq!(st, StatusCode::OK);
    let (st, _) = call(&env, Method::GET, &format!("/api/products/{syrup_id}"), Some(t), None).await;
    assert_eq!(st, StatusCode::NOT_FOUND);

    // единица используется молоком — удалить нельзя; категория удаляется, у товаров сбрасывается
    let (st, _) = call(&env, Method::DELETE, &format!("/api/product-units/{piece}"), Some(t), None).await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
    let (st, _) = call(&env, Method::DELETE, &format!("/api/product-categories/{cat_id}"), Some(t), None).await;
    assert_eq!(st, StatusCode::OK);
    let (_, milk) = call(&env, Method::GET, &format!("/api/products/{milk_id}"), Some(t), None).await;
    assert!(milk["categoryId"].is_null());

    // права: продавец не редактирует справочники и не удаляет товары
    let (_, roles) = call(&env, Method::GET, "/api/roles", Some(t), None).await;
    let cashier = roles.as_array().unwrap().iter().find(|r| r["key"] == "cashier").unwrap()["id"].as_str().unwrap().to_string();
    let seller_email = format!("seller-{}@example.com", uuid_suffix());
    let (st, _) = call(&env, Method::POST, "/api/users", Some(t), Some(json!({
        "name": "S", "email": seller_email, "password": "pass123", "roleId": cashier, "storeIds": [store1]
    }))).await;
    assert_eq!(st, StatusCode::OK);
    let (_, login) = call(&env, Method::POST, "/api/auth/login", None, Some(json!({"email": seller_email, "password": "pass123"}))).await;
    let st_token = login["token"].as_str().unwrap().to_string();
    let (st, _) = call(&env, Method::POST, "/api/suppliers", Some(&st_token), Some(json!({"name": "Z"}))).await;
    assert_eq!(st, StatusCode::FORBIDDEN);
    let (st, _) = call(&env, Method::DELETE, &format!("/api/products/{milk_id}"), Some(&st_token), None).await;
    assert_eq!(st, StatusCode::FORBIDDEN);
    // без токена — 401
    let (st, _) = call(&env, Method::GET, "/api/products", None, None).await;
    assert_eq!(st, StatusCode::UNAUTHORIZED);
}

fn uuid_suffix() -> String {
    uuid::Uuid::new_v4().simple().to_string()[..8].to_string()
}
