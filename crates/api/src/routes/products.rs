//! /api/products — товары, услуги и ингредиенты
use crate::{
    auth::AuthContext,
    error::{AppError, AppResult},
    AppState,
};
use atlas_domain::catalog::{creates_cycle, unit_cost, Node};
use axum::{
    extract::{Path, Query, State},
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, Postgres, Transaction};
use std::collections::HashMap;
use uuid::Uuid;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list).post(create))
        .route("/{id}", get(one).put(update).delete(remove))
        .route("/{id}/copy", post(copy))
}

const SELECT_PRODUCTS: &str = "SELECT p.id, p.name, p.is_service, p.is_composite, p.production_type, p.hidden_in_new_sale, \
     p.purchase_price::float8 AS purchase_price, p.extra_cost::float8 AS extra_cost, p.category_id, p.supplier_id, p.unit_id, \
     p.barcodes, p.allow_fractional, p.important, p.stock_threshold::float8 AS stock_threshold \
     FROM products p \
     WHERE p.organization_id = $1 AND p.deleted_at IS NULL \
       AND ($2::text IS NULL OR p.name ILIKE '%' || $2 || '%' OR $2 = ANY(p.barcodes)) \
       AND ($3::uuid IS NULL OR p.category_id = $3) \
       AND ($4::uuid IS NULL OR p.id = $4) \
       AND ($5::uuid IS NULL OR $6 OR NOT EXISTS \
            (SELECT 1 FROM product_stores ps WHERE ps.product_id = p.id AND ps.store_id = $5 AND NOT ps.active)) \
     ORDER BY p.name";

#[derive(FromRow)]
struct ProductRow {
    id: Uuid,
    name: String,
    is_service: bool,
    is_composite: bool,
    production_type: String,
    hidden_in_new_sale: bool,
    purchase_price: f64,
    extra_cost: f64,
    category_id: Option<Uuid>,
    supplier_id: Option<Uuid>,
    unit_id: Option<Uuid>,
    barcodes: Vec<String>,
    allow_fractional: bool,
    important: bool,
    stock_threshold: f64,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct StorePrice {
    pub store_id: Uuid,
    #[serde(default = "yes")]
    pub active: bool,
    #[serde(default)]
    pub sale_price: f64,
}

fn yes() -> bool {
    true
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct IngredientDto {
    pub product_id: Uuid,
    pub qty: f64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IngredientView {
    pub product_id: Uuid,
    pub name: String,
    pub qty: f64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProductView {
    pub id: Uuid,
    pub name: String,
    pub is_service: bool,
    pub is_composite: bool,
    pub production_type: String,
    pub hidden_in_new_sale: bool,
    pub purchase_price: f64,
    pub extra_cost: f64,
    /// Себестоимость 1 шт (для составных считается из состава).
    pub cost_price: f64,
    pub category_id: Option<Uuid>,
    pub supplier_id: Option<Uuid>,
    pub unit_id: Option<Uuid>,
    pub barcodes: Vec<String>,
    pub allow_fractional: bool,
    pub important: bool,
    pub stock_threshold: f64,
    pub stores: Vec<StorePrice>,
    pub ingredients: Vec<IngredientView>,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct ProductDto {
    pub name: String,
    pub is_service: bool,
    pub is_composite: bool,
    pub production_type: Option<String>,
    pub hidden_in_new_sale: bool,
    pub purchase_price: f64,
    pub extra_cost: f64,
    pub category_id: Option<Uuid>,
    pub supplier_id: Option<Uuid>,
    pub unit_id: Option<Uuid>,
    pub barcodes: Vec<String>,
    pub allow_fractional: bool,
    pub important: bool,
    pub stock_threshold: f64,
    pub stores: Vec<StorePrice>,
    pub ingredients: Vec<IngredientDto>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListQuery {
    pub search: Option<String>,
    pub category_id: Option<Uuid>,
    pub store_id: Option<Uuid>,
    #[serde(default)]
    pub include_inactive: bool,
}

/// Себестоимость: узлы всех товаров организации.
async fn cost_nodes(pool: &sqlx::PgPool, org_id: Uuid) -> AppResult<HashMap<Uuid, Node<Uuid>>> {
    let prods: Vec<(Uuid, f64, f64, bool)> = sqlx::query_as(
        "SELECT id, purchase_price::float8, extra_cost::float8, is_composite FROM products WHERE organization_id=$1 AND deleted_at IS NULL",
    )
    .bind(org_id)
    .fetch_all(pool)
    .await?;
    let edges: Vec<(Uuid, Uuid, f64)> = sqlx::query_as(
        "SELECT i.product_id, i.ingredient_id, i.qty::float8 FROM product_ingredients i \
         JOIN products p ON p.id = i.product_id WHERE p.organization_id=$1 AND p.deleted_at IS NULL",
    )
    .bind(org_id)
    .fetch_all(pool)
    .await?;
    let mut map: HashMap<Uuid, Node<Uuid>> = prods
        .into_iter()
        .map(|(id, pp, ec, comp)| (id, Node { purchase_price: pp, extra_cost: ec, composite: comp, ingredients: vec![] }))
        .collect();
    for (p, i, q) in edges {
        if let Some(n) = map.get_mut(&p) {
            n.ingredients.push((i, q));
        }
    }
    Ok(map)
}

async fn fetch_views(
    s: &AppState,
    ctx: &AuthContext,
    search: Option<String>,
    category: Option<Uuid>,
    only_id: Option<Uuid>,
    store: Option<Uuid>,
    include_inactive: bool,
) -> AppResult<Vec<ProductView>> {
    let search = search.map(|v| v.trim().to_string()).filter(|v| !v.is_empty());
    let show_inactive = include_inactive && ctx.can("product.inactive");
    let rows = sqlx::query_as::<_, ProductRow>(SELECT_PRODUCTS)
        .bind(ctx.org_id)
        .bind(search)
        .bind(category)
        .bind(only_id)
        .bind(store)
        .bind(show_inactive)
        .fetch_all(&s.pool)
        .await?;
    let ids: Vec<Uuid> = rows.iter().map(|r| r.id).collect();

    let store_rows: Vec<(Uuid, Uuid, bool, f64)> = sqlx::query_as(
        "SELECT product_id, store_id, active, sale_price::float8 FROM product_stores WHERE product_id = ANY($1) ORDER BY store_id",
    )
    .bind(&ids)
    .fetch_all(&s.pool)
    .await?;
    let mut stores: HashMap<Uuid, Vec<StorePrice>> = HashMap::new();
    for (pid, store_id, active, sale_price) in store_rows {
        stores.entry(pid).or_default().push(StorePrice { store_id, active, sale_price });
    }

    let ing_rows: Vec<(Uuid, Uuid, String, f64)> = sqlx::query_as(
        "SELECT i.product_id, i.ingredient_id, p.name, i.qty::float8 FROM product_ingredients i \
         JOIN products p ON p.id = i.ingredient_id WHERE i.product_id = ANY($1) ORDER BY p.name",
    )
    .bind(&ids)
    .fetch_all(&s.pool)
    .await?;
    let mut ings: HashMap<Uuid, Vec<IngredientView>> = HashMap::new();
    for (pid, product_id, name, qty) in ing_rows {
        ings.entry(pid).or_default().push(IngredientView { product_id, name, qty });
    }

    let nodes = cost_nodes(&s.pool, ctx.org_id).await?;
    Ok(rows
        .into_iter()
        .map(|r| ProductView {
            cost_price: unit_cost(&nodes, r.id),
            stores: stores.remove(&r.id).unwrap_or_default(),
            ingredients: ings.remove(&r.id).unwrap_or_default(),
            id: r.id,
            name: r.name,
            is_service: r.is_service,
            is_composite: r.is_composite,
            production_type: r.production_type,
            hidden_in_new_sale: r.hidden_in_new_sale,
            purchase_price: r.purchase_price,
            extra_cost: r.extra_cost,
            category_id: r.category_id,
            supplier_id: r.supplier_id,
            unit_id: r.unit_id,
            barcodes: r.barcodes,
            allow_fractional: r.allow_fractional,
            important: r.important,
            stock_threshold: r.stock_threshold,
        })
        .collect())
}

async fn list(State(s): State<AppState>, ctx: AuthContext, Query(q): Query<ListQuery>) -> AppResult<Json<Vec<ProductView>>> {
    ctx.require("product.view")?;
    Ok(Json(fetch_views(&s, &ctx, q.search, q.category_id, None, q.store_id, q.include_inactive).await?))
}

async fn one(State(s): State<AppState>, ctx: AuthContext, Path(id): Path<Uuid>) -> AppResult<Json<ProductView>> {
    ctx.require("product.view")?;
    fetch_views(&s, &ctx, None, None, Some(id), None, true)
        .await?
        .into_iter()
        .next()
        .map(Json)
        .ok_or_else(|| AppError::NotFound("Товар не найден".into()))
}

fn price_ok(v: f64) -> bool {
    v.is_finite() && v >= 0.0
}

/// Нормализация и проверка входных данных без обращения к БД.
fn normalize(dto: &mut ProductDto) -> AppResult<()> {
    dto.name = dto.name.trim().to_string();
    if dto.name.is_empty() {
        return Err(AppError::BadRequest("Введите название".into()));
    }
    if !price_ok(dto.purchase_price) || !price_ok(dto.extra_cost) || !price_ok(dto.stock_threshold) {
        return Err(AppError::BadRequest("Цена и порог не могут быть отрицательными".into()));
    }
    if dto.stores.iter().any(|s| !price_ok(s.sale_price)) {
        return Err(AppError::BadRequest("Цена продажи не может быть отрицательной".into()));
    }
    match dto.production_type.as_deref() {
        None | Some("ahead") | Some("on_sale") => {}
        Some(_) => return Err(AppError::BadRequest("Неизвестный тип производства".into())),
    }
    if dto.is_service && dto.is_composite {
        return Err(AppError::BadRequest("Услуга не может быть составной".into()));
    }
    if !dto.is_composite && !dto.ingredients.is_empty() {
        return Err(AppError::BadRequest("Состав задаётся только у составных товаров".into()));
    }
    if dto.ingredients.iter().any(|i| !(i.qty.is_finite() && i.qty > 0.0)) {
        return Err(AppError::BadRequest("Количество ингредиента должно быть больше нуля".into()));
    }
    let mut seen = std::collections::HashSet::new();
    if !dto.ingredients.iter().all(|i| seen.insert(i.product_id)) {
        return Err(AppError::BadRequest("Ингредиент указан дважды".into()));
    }
    let mut bc: Vec<String> = Vec::new();
    for b in dto.barcodes.drain(..) {
        let b = b.trim().to_string();
        if !b.is_empty() && !bc.contains(&b) {
            bc.push(b);
        }
    }
    dto.barcodes = bc;
    Ok(())
}

/// Проверки, требующие БД; пишет товар и связанные строки внутри транзакции.
async fn save(
    tx: &mut Transaction<'_, Postgres>,
    pool: &sqlx::PgPool,
    org_id: Uuid,
    id: Uuid,
    dto: &ProductDto,
    creating: bool,
) -> AppResult<()> {
    // справочники принадлежат организации
    if let Some(c) = dto.category_id {
        let (n,): (i64,) = sqlx::query_as("SELECT count(*) FROM product_categories WHERE id=$1 AND organization_id=$2 AND deleted_at IS NULL")
            .bind(c)
            .bind(org_id)
            .fetch_one(&mut **tx)
            .await?;
        if n == 0 {
            return Err(AppError::BadRequest("Категория не найдена".into()));
        }
    }
    if let Some(c) = dto.supplier_id {
        let (n,): (i64,) = sqlx::query_as("SELECT count(*) FROM suppliers WHERE id=$1 AND organization_id=$2 AND deleted_at IS NULL")
            .bind(c)
            .bind(org_id)
            .fetch_one(&mut **tx)
            .await?;
        if n == 0 {
            return Err(AppError::BadRequest("Поставщик не найден".into()));
        }
    }
    if let Some(c) = dto.unit_id {
        let (n,): (i64,) = sqlx::query_as("SELECT count(*) FROM product_units WHERE id=$1 AND organization_id=$2 AND deleted_at IS NULL")
            .bind(c)
            .bind(org_id)
            .fetch_one(&mut **tx)
            .await?;
        if n == 0 {
            return Err(AppError::BadRequest("Единица измерения не найдена".into()));
        }
    }
    // филиалы
    let store_ids: Vec<Uuid> = dto.stores.iter().map(|s| s.store_id).collect();
    if !store_ids.is_empty() {
        let (n,): (i64,) = sqlx::query_as("SELECT count(*) FROM stores WHERE organization_id=$1 AND deleted_at IS NULL AND id = ANY($2)")
            .bind(org_id)
            .bind(&store_ids)
            .fetch_one(&mut **tx)
            .await?;
        if n as usize != store_ids.iter().collect::<std::collections::HashSet<_>>().len() {
            return Err(AppError::BadRequest("Филиал не найден".into()));
        }
    }
    // ингредиенты: существуют, не услуги, без циклов
    if !dto.ingredients.is_empty() {
        let ids: Vec<Uuid> = dto.ingredients.iter().map(|i| i.product_id).collect();
        let (n,): (i64,) = sqlx::query_as(
            "SELECT count(*) FROM products WHERE organization_id=$1 AND deleted_at IS NULL AND NOT is_service AND id = ANY($2)",
        )
        .bind(org_id)
        .bind(&ids)
        .fetch_one(&mut **tx)
        .await?;
        if n as usize != ids.len() {
            return Err(AppError::BadRequest("Ингредиент не найден или является услугой".into()));
        }
        let nodes = cost_nodes(pool, org_id).await?;
        if creates_cycle(&nodes, id, &ids) {
            return Err(AppError::BadRequest("Ингредиент создаёт цикл в составе".into()));
        }
    }
    // штрихкоды уникальны в организации
    if !dto.barcodes.is_empty() {
        let (n,): (i64,) = sqlx::query_as(
            "SELECT count(*) FROM products WHERE organization_id=$1 AND deleted_at IS NULL AND id <> $2 AND barcodes && $3",
        )
        .bind(org_id)
        .bind(id)
        .bind(&dto.barcodes)
        .fetch_one(&mut **tx)
        .await?;
        if n > 0 {
            return Err(AppError::Conflict("Штрихкод уже используется".into()));
        }
    }

    let production = dto.production_type.clone().unwrap_or_else(|| "ahead".into());
    if creating {
        sqlx::query(
            "INSERT INTO products (id, organization_id, name, is_service, is_composite, production_type, hidden_in_new_sale, \
             purchase_price, extra_cost, category_id, supplier_id, unit_id, barcodes, allow_fractional, important, stock_threshold) \
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8::float8,$9::float8,$10,$11,$12,$13,$14,$15,$16::float8)",
        )
        .bind(id)
        .bind(org_id)
        .bind(&dto.name)
        .bind(dto.is_service)
        .bind(dto.is_composite)
        .bind(&production)
        .bind(dto.hidden_in_new_sale)
        .bind(dto.purchase_price)
        .bind(dto.extra_cost)
        .bind(dto.category_id)
        .bind(dto.supplier_id)
        .bind(dto.unit_id)
        .bind(&dto.barcodes)
        .bind(dto.allow_fractional)
        .bind(dto.important)
        .bind(dto.stock_threshold)
        .execute(&mut **tx)
        .await?;
    } else {
        let res = sqlx::query(
            "UPDATE products SET name=$3, is_service=$4, is_composite=$5, production_type=$6, hidden_in_new_sale=$7, \
             purchase_price=$8::float8, extra_cost=$9::float8, category_id=$10, supplier_id=$11, unit_id=$12, barcodes=$13, \
             allow_fractional=$14, important=$15, stock_threshold=$16::float8 \
             WHERE id=$1 AND organization_id=$2 AND deleted_at IS NULL",
        )
        .bind(id)
        .bind(org_id)
        .bind(&dto.name)
        .bind(dto.is_service)
        .bind(dto.is_composite)
        .bind(&production)
        .bind(dto.hidden_in_new_sale)
        .bind(dto.purchase_price)
        .bind(dto.extra_cost)
        .bind(dto.category_id)
        .bind(dto.supplier_id)
        .bind(dto.unit_id)
        .bind(&dto.barcodes)
        .bind(dto.allow_fractional)
        .bind(dto.important)
        .bind(dto.stock_threshold)
        .execute(&mut **tx)
        .await?;
        if res.rows_affected() == 0 {
            return Err(AppError::NotFound("Товар не найден".into()));
        }
    }

    for sp in &dto.stores {
        sqlx::query(
            "INSERT INTO product_stores (product_id, store_id, active, sale_price) VALUES ($1,$2,$3,$4::float8) \
             ON CONFLICT (product_id, store_id) DO UPDATE SET active = EXCLUDED.active, sale_price = EXCLUDED.sale_price",
        )
        .bind(id)
        .bind(sp.store_id)
        .bind(sp.active)
        .bind(sp.sale_price)
        .execute(&mut **tx)
        .await?;
    }
    // остальные филиалы организации — по умолчанию включён, цена 0
    sqlx::query(
        "INSERT INTO product_stores (product_id, store_id, active, sale_price) \
         SELECT $1, st.id, true, 0 FROM stores st WHERE st.organization_id=$2 AND st.deleted_at IS NULL \
         ON CONFLICT (product_id, store_id) DO NOTHING",
    )
    .bind(id)
    .bind(org_id)
    .execute(&mut **tx)
    .await?;

    sqlx::query("DELETE FROM product_ingredients WHERE product_id=$1").bind(id).execute(&mut **tx).await?;
    for ing in &dto.ingredients {
        sqlx::query("INSERT INTO product_ingredients (product_id, ingredient_id, qty) VALUES ($1,$2,$3::float8)")
            .bind(id)
            .bind(ing.product_id)
            .bind(ing.qty)
            .execute(&mut **tx)
            .await?;
    }
    Ok(())
}

async fn create(State(s): State<AppState>, ctx: AuthContext, Json(mut dto): Json<ProductDto>) -> AppResult<Json<ProductView>> {
    ctx.require("product.view")?;
    normalize(&mut dto)?;
    let id = Uuid::new_v4();
    let mut tx = s.pool.begin().await?;
    save(&mut tx, &s.pool, ctx.org_id, id, &dto, true).await?;
    tx.commit().await?;
    one(State(s), ctx, Path(id)).await
}

async fn update(
    State(s): State<AppState>,
    ctx: AuthContext,
    Path(id): Path<Uuid>,
    Json(mut dto): Json<ProductDto>,
) -> AppResult<Json<ProductView>> {
    ctx.require("product.view")?;
    normalize(&mut dto)?;
    let mut tx = s.pool.begin().await?;
    save(&mut tx, &s.pool, ctx.org_id, id, &dto, false).await?;
    tx.commit().await?;
    one(State(s), ctx, Path(id)).await
}

/// «Создать копию»: те же поля, название с пометкой, штрихкоды не копируются (должны быть уникальны).
async fn copy(State(s): State<AppState>, ctx: AuthContext, Path(id): Path<Uuid>) -> AppResult<Json<ProductView>> {
    ctx.require("product.view")?;
    let src = fetch_views(&s, &ctx, None, None, Some(id), None, true)
        .await?
        .into_iter()
        .next()
        .ok_or_else(|| AppError::NotFound("Товар не найден".into()))?;
    let mut dto = ProductDto {
        name: format!("{} (копия)", src.name),
        is_service: src.is_service,
        is_composite: src.is_composite,
        production_type: Some(src.production_type),
        hidden_in_new_sale: src.hidden_in_new_sale,
        purchase_price: src.purchase_price,
        extra_cost: src.extra_cost,
        category_id: src.category_id,
        supplier_id: src.supplier_id,
        unit_id: src.unit_id,
        barcodes: vec![],
        allow_fractional: src.allow_fractional,
        important: src.important,
        stock_threshold: src.stock_threshold,
        stores: src.stores,
        ingredients: src.ingredients.into_iter().map(|i| IngredientDto { product_id: i.product_id, qty: i.qty }).collect(),
    };
    normalize(&mut dto)?;
    let new_id = Uuid::new_v4();
    let mut tx = s.pool.begin().await?;
    save(&mut tx, &s.pool, ctx.org_id, new_id, &dto, true).await?;
    tx.commit().await?;
    one(State(s), ctx, Path(new_id)).await
}

async fn remove(State(s): State<AppState>, ctx: AuthContext, Path(id): Path<Uuid>) -> AppResult<Json<serde_json::Value>> {
    ctx.require("product.delete")?;
    let (used,): (i64,) = sqlx::query_as(
        "SELECT count(*) FROM product_ingredients i JOIN products p ON p.id = i.product_id AND p.deleted_at IS NULL \
         WHERE i.ingredient_id = $1",
    )
    .bind(id)
    .fetch_one(&s.pool)
    .await?;
    if used > 0 {
        return Err(AppError::BadRequest("Товар используется как ингредиент".into()));
    }
    let res = sqlx::query("UPDATE products SET deleted_at=now() WHERE id=$1 AND organization_id=$2 AND deleted_at IS NULL")
        .bind(id)
        .bind(ctx.org_id)
        .execute(&s.pool)
        .await?;
    if res.rows_affected() == 0 {
        return Err(AppError::NotFound("Товар не найден".into()));
    }
    Ok(Json(serde_json::json!({ "success": true })))
}
