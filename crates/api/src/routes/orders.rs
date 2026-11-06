//! Заказы клиентов (/api/orders) и публичный онлайн-каталог (/api/marketplace/*, POST /api/orders/public)
use crate::{
    auth::AuthContext,
    error::{AppError, AppResult},
    routes::sales::{self, CreateSaleDto, ItemDto as SaleItemDto},
    AppState,
};
use atlas_domain::sale::round2;
use axum::{
    extract::{Path, Query, State},
    routing::{get, patch, post},
    Json, Router,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::FromRow;
use uuid::Uuid;

pub const STATUSES: [&str; 5] = ["new", "in_work", "ready", "done", "cancelled"];

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list).post(create))
        .route("/public", post(public_create))
        .route("/count/unprocessed", get(count_unprocessed))
        .route("/{id}", get(one).patch(update))
        .route("/{id}/status", patch(set_status))
        .route("/{id}/create-sale", post(create_sale))
}

pub fn marketplace_router() -> Router<AppState> {
    Router::new()
        .route("/organizations", get(mp_organizations))
        .route("/organizations/{id}", get(mp_organization))
        .route("/product-categories", get(mp_categories))
        .route("/products", get(mp_products))
        .route("/product-units", get(mp_units))
}

#[derive(Serialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct OrderView {
    pub id: Uuid,
    pub number: i64,
    pub store_id: Uuid,
    pub source: String,
    pub client_id: Option<Uuid>,
    pub client_name: String,
    pub client_phone: String,
    pub client_email: String,
    pub items: Value,
    pub total: f64,
    pub status: String,
    pub payment_method: String,
    pub delivery_address: String,
    pub notes: String,
    pub sale_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
}

const SELECT_ORDERS: &str = "SELECT id, number, store_id, source, client_id, client_name, client_phone, client_email, items, \
     total::float8 AS total, status, payment_method, delivery_address, notes, sale_id, created_at FROM client_orders \
     WHERE organization_id=$1 AND deleted_at IS NULL AND ($2::uuid IS NULL OR id=$2) AND ($3::uuid IS NULL OR store_id=$3) \
       AND ($4::timestamptz IS NULL OR created_at >= $4) AND ($5::timestamptz IS NULL OR created_at < $5) \
       AND (NOT $6 OR status IN ('new','in_work','ready')) ORDER BY created_at DESC LIMIT 500";

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ListQuery {
    pub store_id: Option<Uuid>,
    pub from: Option<DateTime<Utc>>,
    pub to: Option<DateTime<Utc>>,
    #[serde(default)]
    pub active_only: bool,
}

async fn fetch(s: &AppState, org: Uuid, id: Option<Uuid>, q: &ListQuery) -> AppResult<Vec<OrderView>> {
    Ok(sqlx::query_as::<_, OrderView>(SELECT_ORDERS)
        .bind(org)
        .bind(id)
        .bind(q.store_id)
        .bind(q.from)
        .bind(q.to)
        .bind(q.active_only)
        .fetch_all(&s.pool)
        .await?)
}

async fn list(State(s): State<AppState>, ctx: AuthContext, Query(q): Query<ListQuery>) -> AppResult<Json<Vec<OrderView>>> {
    ctx.require("client_orders.view")?;
    Ok(Json(fetch(&s, ctx.org_id, None, &q).await?))
}

async fn one(State(s): State<AppState>, ctx: AuthContext, Path(id): Path<Uuid>) -> AppResult<Json<OrderView>> {
    ctx.require("client_orders.view")?;
    fetch(&s, ctx.org_id, Some(id), &ListQuery::default())
        .await?
        .into_iter()
        .next()
        .map(Json)
        .ok_or_else(|| AppError::NotFound("Заказ не найден".into()))
}

async fn count_unprocessed(State(s): State<AppState>, ctx: AuthContext) -> AppResult<Json<Value>> {
    ctx.require("client_orders.view")?;
    let (n,): (i64,) = sqlx::query_as("SELECT count(*) FROM client_orders WHERE organization_id=$1 AND deleted_at IS NULL AND status='new'")
        .bind(ctx.org_id)
        .fetch_one(&s.pool)
        .await?;
    Ok(Json(json!({ "count": n })))
}

// ---------- позиции заказа ----------
#[derive(Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct OrderItemDto {
    pub product_id: Option<Uuid>,
    pub name: Option<String>,
    pub qty: f64,
    pub price: Option<f64>,
}

/// Проверка позиций: товар существует, цена берётся из филиала (если не задана), считается итог.
async fn resolve_items(s: &AppState, org: Uuid, store: Uuid, items: &[OrderItemDto], public: bool) -> AppResult<(Value, f64)> {
    if items.is_empty() {
        return Err(AppError::BadRequest("Добавьте товары в заказ".into()));
    }
    let pids: Vec<Uuid> = items.iter().filter_map(|i| i.product_id).collect();
    let rows: Vec<(Uuid, String, bool, bool, f64)> = sqlx::query_as(
        "SELECT p.id, p.name, p.is_service, COALESCE(ps.active, true), COALESCE(ps.sale_price,0)::float8 FROM products p \
         LEFT JOIN product_stores ps ON ps.product_id=p.id AND ps.store_id=$3 \
         WHERE p.organization_id=$1 AND p.deleted_at IS NULL AND p.id = ANY($2)",
    )
    .bind(org)
    .bind(&pids)
    .bind(store)
    .fetch_all(&s.pool)
    .await?;
    let mut out = Vec::new();
    let mut total = 0.0;
    for it in items {
        if !(it.qty.is_finite() && it.qty > 0.0) {
            return Err(AppError::BadRequest("Количество должно быть больше нуля".into()));
        }
        let (name, price) = match it.product_id {
            Some(pid) => {
                let r = rows.iter().find(|r| r.0 == pid).ok_or_else(|| AppError::BadRequest("Товар не найден".into()))?;
                if !r.3 {
                    return Err(AppError::BadRequest(format!("Товар «{}» недоступен", r.1)));
                }
                // публичный заказ не может задавать собственные цены
                let price = if public { r.4 } else { it.price.unwrap_or(r.4) };
                (r.1.clone(), price)
            }
            None => {
                if public {
                    return Err(AppError::BadRequest("В онлайн-заказе только товары каталога".into()));
                }
                let n = it.name.as_deref().map(str::trim).filter(|n| !n.is_empty());
                (
                    n.ok_or_else(|| AppError::BadRequest("Введите название позиции".into()))?.to_string(),
                    it.price.ok_or_else(|| AppError::BadRequest("Укажите цену позиции".into()))?,
                )
            }
        };
        if !(price.is_finite() && price >= 0.0) {
            return Err(AppError::BadRequest("Цена не может быть отрицательной".into()));
        }
        total += it.qty * price;
        out.push(json!({ "productId": it.product_id, "name": name, "qty": it.qty, "price": price }));
    }
    Ok((Value::Array(out), round2(total)))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrderDto {
    pub store_id: Uuid,
    pub client_id: Option<Uuid>,
    pub client_name: Option<String>,
    pub client_phone: Option<String>,
    pub client_email: Option<String>,
    pub items: Vec<OrderItemDto>,
    pub payment_method: Option<String>,
    pub delivery_address: Option<String>,
    pub notes: Option<String>,
}

async fn create(State(s): State<AppState>, ctx: AuthContext, Json(dto): Json<OrderDto>) -> AppResult<Json<OrderView>> {
    ctx.require("client_orders.view")?;
    sales::check_store(&s.pool, &ctx, dto.store_id).await?;
    let (items, total) = resolve_items(&s, ctx.org_id, dto.store_id, &dto.items, false).await?;
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO client_orders (id, organization_id, store_id, source, client_id, client_name, client_phone, client_email, items, \
         total, payment_method, delivery_address, notes) VALUES ($1,$2,$3,'manual',$4,$5,$6,$7,$8,$9::float8,$10,$11,$12)",
    )
    .bind(id)
    .bind(ctx.org_id)
    .bind(dto.store_id)
    .bind(dto.client_id)
    .bind(dto.client_name.unwrap_or_default())
    .bind(dto.client_phone.unwrap_or_default())
    .bind(dto.client_email.unwrap_or_default())
    .bind(items)
    .bind(total)
    .bind(dto.payment_method.unwrap_or_default())
    .bind(dto.delivery_address.unwrap_or_default())
    .bind(dto.notes.unwrap_or_default())
    .execute(&s.pool)
    .await?;
    one(State(s), ctx, Path(id)).await
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrderPatch {
    pub client_name: Option<String>,
    pub client_phone: Option<String>,
    pub client_email: Option<String>,
    pub payment_method: Option<String>,
    pub delivery_address: Option<String>,
    pub notes: Option<String>,
}

async fn update(
    State(s): State<AppState>,
    ctx: AuthContext,
    Path(id): Path<Uuid>,
    Json(p): Json<OrderPatch>,
) -> AppResult<Json<OrderView>> {
    ctx.require("client_orders.view")?;
    let res = sqlx::query(
        "UPDATE client_orders SET client_name=COALESCE($3, client_name), client_phone=COALESCE($4, client_phone), \
         client_email=COALESCE($5, client_email), payment_method=COALESCE($6, payment_method), \
         delivery_address=COALESCE($7, delivery_address), notes=COALESCE($8, notes), updated_at=now() \
         WHERE id=$1 AND organization_id=$2 AND deleted_at IS NULL",
    )
    .bind(id)
    .bind(ctx.org_id)
    .bind(p.client_name)
    .bind(p.client_phone)
    .bind(p.client_email)
    .bind(p.payment_method)
    .bind(p.delivery_address)
    .bind(p.notes)
    .execute(&s.pool)
    .await?;
    if res.rows_affected() == 0 {
        return Err(AppError::NotFound("Заказ не найден".into()));
    }
    one(State(s), ctx, Path(id)).await
}

/// Новый → В работе → Готов → Выполнен; «Отменён» — из любого незавершённого, из отменённого можно вернуть в «Новый».
fn transition_ok(from: &str, to: &str) -> bool {
    let rank = |s: &str| STATUSES.iter().position(|x| *x == s);
    match (from, to) {
        ("done", _) => false,
        ("cancelled", "new") => true,
        ("cancelled", _) => false,
        (_, "cancelled") => true,
        (f, t) => matches!((rank(f), rank(t)), (Some(a), Some(b)) if b > a && b <= 3),
    }
}

#[derive(Deserialize)]
pub struct StatusDto {
    pub status: String,
}

async fn set_status(
    State(s): State<AppState>,
    ctx: AuthContext,
    Path(id): Path<Uuid>,
    Json(dto): Json<StatusDto>,
) -> AppResult<Json<OrderView>> {
    ctx.require("client_orders.view")?;
    if !STATUSES.contains(&dto.status.as_str()) {
        return Err(AppError::BadRequest("Неизвестный статус заказа".into()));
    }
    let cur: Option<(String,)> = sqlx::query_as("SELECT status FROM client_orders WHERE id=$1 AND organization_id=$2 AND deleted_at IS NULL")
        .bind(id)
        .bind(ctx.org_id)
        .fetch_optional(&s.pool)
        .await?;
    let (from,) = cur.ok_or_else(|| AppError::NotFound("Заказ не найден".into()))?;
    if !transition_ok(&from, &dto.status) {
        return Err(AppError::BadRequest("Такой переход статуса недоступен".into()));
    }
    sqlx::query("UPDATE client_orders SET status=$2, updated_at=now() WHERE id=$1").bind(id).bind(&dto.status).execute(&s.pool).await?;
    one(State(s), ctx, Path(id)).await
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ToSaleDto {
    pub payment_method_id: Option<Uuid>,
}

/// «Создать продажу»: позиции заказа становятся чеком; заказ получает статус «Выполнен» и ссылку на продажу.
async fn create_sale(
    State(s): State<AppState>,
    ctx: AuthContext,
    Path(id): Path<Uuid>,
    body: Option<Json<ToSaleDto>>,
) -> AppResult<Json<sales::SaleView>> {
    ctx.require("client_orders.view")?;
    let order = fetch(&s, ctx.org_id, Some(id), &ListQuery::default())
        .await?
        .into_iter()
        .next()
        .ok_or_else(|| AppError::NotFound("Заказ не найден".into()))?;
    if order.sale_id.is_some() || order.status == "done" || order.status == "cancelled" {
        return Err(AppError::BadRequest("По этому заказу нельзя создать продажу".into()));
    }
    let items: Vec<SaleItemDto> = order
        .items
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .map(|i| SaleItemDto {
            product_id: i.get("productId").and_then(Value::as_str).and_then(|v| Uuid::parse_str(v).ok()),
            name: i.get("name").and_then(Value::as_str).map(str::to_string),
            qty: i.get("qty").and_then(Value::as_f64).unwrap_or(0.0),
            price: i.get("price").and_then(Value::as_f64),
            purchase_price: None,
        })
        .collect();
    // клиент заказа — по телефону
    let client_id = match order.client_id {
        Some(c) => Some(c),
        None if !order.client_phone.is_empty() => {
            let r: Option<(Uuid,)> =
                sqlx::query_as("SELECT id FROM clients WHERE organization_id=$1 AND phone=$2 AND deleted_at IS NULL")
                    .bind(ctx.org_id)
                    .bind(&order.client_phone)
                    .fetch_optional(&s.pool)
                    .await?;
            r.map(|x| x.0)
        }
        None => None,
    };
    let dto = CreateSaleDto {
        store_id: order.store_id,
        client_id,
        items,
        payment_method_id: body.and_then(|b| b.0.payment_method_id),
        delivery: !order.delivery_address.is_empty(),
        delivery_address: Some(order.delivery_address.clone()),
        delivery_phone: Some(order.client_phone.clone()),
        comment: Some(format!("Заказ №{}", order.number)),
        order_id: Some(id),
        idempotency_key: Some(format!("order-{id}")),
        ..Default::default()
    };
    let Json(sale) = sales::create(State(s.clone()), ctx.clone(), Json(dto)).await?;
    sqlx::query("UPDATE client_orders SET status='done', sale_id=$2, updated_at=now() WHERE id=$1")
        .bind(id)
        .bind(sale.id)
        .execute(&s.pool)
        .await?;
    Ok(Json(sale))
}

// =========================== публичный каталог ===========================
async fn marketplace_cfg(s: &AppState, org: Uuid) -> AppResult<(String, Value)> {
    let row: Option<(String, Value)> = sqlx::query_as("SELECT name, marketplace FROM organizations WHERE id=$1 AND deleted_at IS NULL")
        .bind(org)
        .fetch_optional(&s.pool)
        .await?;
    let (name, cfg) = row.ok_or_else(|| AppError::NotFound("Организация не найдена".into()))?;
    if !cfg.get("marketplaceEnabled").and_then(Value::as_bool).unwrap_or(false) {
        return Err(AppError::NotFound("Онлайн-каталог выключен".into()));
    }
    Ok((name, cfg))
}

fn mp_store(cfg: &Value) -> AppResult<Uuid> {
    cfg.get("storeForMarketplace")
        .and_then(Value::as_str)
        .and_then(|v| Uuid::parse_str(v).ok())
        .ok_or_else(|| AppError::BadRequest("Для онлайн-каталога не выбран филиал".into()))
}

async fn mp_organizations(State(s): State<AppState>) -> AppResult<Json<Vec<Value>>> {
    let rows: Vec<(Uuid, String, Value)> = sqlx::query_as(
        "SELECT id, name, marketplace FROM organizations WHERE deleted_at IS NULL AND (marketplace->>'marketplaceEnabled')::boolean ORDER BY name",
    )
    .fetch_all(&s.pool)
    .await?;
    Ok(Json(
        rows.into_iter()
            .map(|(id, name, m)| json!({ "id": id, "name": name, "description": m.get("description").cloned().unwrap_or(json!("")) }))
            .collect(),
    ))
}

async fn mp_organization(State(s): State<AppState>, Path(id): Path<Uuid>) -> AppResult<Json<Value>> {
    let (name, cfg) = marketplace_cfg(&s, id).await?;
    Ok(Json(json!({ "id": id, "name": name, "description": cfg.get("description").cloned().unwrap_or(json!("")) })))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrgQuery {
    pub organization_id: Uuid,
    pub category_id: Option<Uuid>,
    pub search: Option<String>,
}

/// Категории, включённые в онлайн-каталоге (пустой список настройки = все).
fn category_enabled(cfg: &Value, id: Option<Uuid>) -> bool {
    let map = cfg.get("enabledCategories").and_then(Value::as_object);
    match (map, id) {
        (Some(m), Some(c)) if !m.is_empty() => m.get(&c.to_string()).and_then(Value::as_bool).unwrap_or(false),
        (Some(m), None) if !m.is_empty() => false,
        _ => true,
    }
}

async fn mp_categories(State(s): State<AppState>, Query(q): Query<OrgQuery>) -> AppResult<Json<Vec<Value>>> {
    let (_, cfg) = marketplace_cfg(&s, q.organization_id).await?;
    let rows: Vec<(Uuid, String)> = sqlx::query_as(
        "SELECT id, name FROM product_categories WHERE organization_id=$1 AND deleted_at IS NULL ORDER BY sort_order, name",
    )
    .bind(q.organization_id)
    .fetch_all(&s.pool)
    .await?;
    Ok(Json(rows.into_iter().filter(|(id, _)| category_enabled(&cfg, Some(*id))).map(|(id, name)| json!({"id": id, "name": name})).collect()))
}

async fn mp_units(State(s): State<AppState>, Query(q): Query<OrgQuery>) -> AppResult<Json<Vec<Value>>> {
    marketplace_cfg(&s, q.organization_id).await?;
    let rows: Vec<(Uuid, String, String)> =
        sqlx::query_as("SELECT id, name, short_name FROM product_units WHERE organization_id=$1 AND deleted_at IS NULL ORDER BY created_at")
            .bind(q.organization_id)
            .fetch_all(&s.pool)
            .await?;
    Ok(Json(rows.into_iter().map(|(id, name, short)| json!({"id": id, "name": name, "shortName": short})).collect()))
}

async fn mp_products(State(s): State<AppState>, Query(q): Query<OrgQuery>) -> AppResult<Json<Vec<Value>>> {
    let (_, cfg) = marketplace_cfg(&s, q.organization_id).await?;
    let store = mp_store(&cfg)?;
    let hide_empty = cfg.get("disableNotInStockProducts").and_then(Value::as_bool).unwrap_or(false);
    let search = q.search.as_deref().map(str::trim).filter(|v| !v.is_empty());
    let rows: Vec<(Uuid, String, Option<Uuid>, Option<Uuid>, bool, f64, f64)> = sqlx::query_as(
        "SELECT p.id, p.name, p.category_id, p.unit_id, p.is_service, ps.sale_price::float8, COALESCE(ss.qty,0)::float8 \
         FROM products p JOIN product_stores ps ON ps.product_id = p.id AND ps.store_id = $2 AND ps.active \
         LEFT JOIN store_stock ss ON ss.product_id = p.id AND ss.store_id = $2 \
         WHERE p.organization_id=$1 AND p.deleted_at IS NULL AND NOT p.hidden_in_new_sale \
           AND ($3::uuid IS NULL OR p.category_id = $3) AND ($4::text IS NULL OR p.name ILIKE '%' || $4 || '%') ORDER BY p.name",
    )
    .bind(q.organization_id)
    .bind(store)
    .bind(q.category_id)
    .bind(search)
    .fetch_all(&s.pool)
    .await?;
    Ok(Json(
        rows.into_iter()
            .filter(|r| category_enabled(&cfg, r.2) && !(hide_empty && !r.4 && r.6 <= 0.0))
            .map(|r| json!({ "id": r.0, "name": r.1, "categoryId": r.2, "unitId": r.3, "price": r.5, "inStock": r.4 || r.6 > 0.0 }))
            .collect(),
    ))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PublicOrderDto {
    pub organization_id: Uuid,
    pub client_name: String,
    pub client_phone: String,
    pub client_email: Option<String>,
    pub items: Vec<OrderItemDto>,
    pub payment_method: Option<String>,
    pub delivery_address: Option<String>,
    pub notes: Option<String>,
}

/// Заказ из онлайн-каталога без авторизации: цены берутся из каталога, итог считается на сервере.
async fn public_create(State(s): State<AppState>, Json(dto): Json<PublicOrderDto>) -> AppResult<Json<Value>> {
    let (_, cfg) = marketplace_cfg(&s, dto.organization_id).await?;
    let store = mp_store(&cfg)?;
    if dto.client_name.trim().is_empty() {
        return Err(AppError::BadRequest("Введите имя".into()));
    }
    if !dto.client_phone.trim().starts_with('+') {
        return Err(AppError::BadRequest("Номер телефона должен начинаться на +".into()));
    }
    let (items, total) = resolve_items(&s, dto.organization_id, store, &dto.items, true).await?;
    if cfg.get("disableNotInStockProducts").and_then(Value::as_bool).unwrap_or(false) {
        for it in &dto.items {
            if let Some(pid) = it.product_id {
                let (qty, service): (f64, bool) = sqlx::query_as(
                    "SELECT COALESCE((SELECT qty::float8 FROM store_stock WHERE product_id=p.id AND store_id=$2),0), p.is_service \
                     FROM products p WHERE p.id=$1",
                )
                .bind(pid)
                .bind(store)
                .fetch_one(&s.pool)
                .await?;
                if !service && qty < it.qty {
                    return Err(AppError::BadRequest("Товара нет в наличии".into()));
                }
            }
        }
    }
    let id = Uuid::new_v4();
    let (number,): (i64,) = sqlx::query_as(
        "INSERT INTO client_orders (id, organization_id, store_id, source, client_name, client_phone, client_email, items, total, \
         payment_method, delivery_address, notes) VALUES ($1,$2,$3,'marketplace',$4,$5,$6,$7,$8::float8,$9,$10,$11) RETURNING number",
    )
    .bind(id)
    .bind(dto.organization_id)
    .bind(store)
    .bind(dto.client_name.trim())
    .bind(dto.client_phone.trim())
    .bind(dto.client_email.unwrap_or_default())
    .bind(items)
    .bind(total)
    .bind(dto.payment_method.unwrap_or_default())
    .bind(dto.delivery_address.unwrap_or_default())
    .bind(dto.notes.unwrap_or_default())
    .fetch_one(&s.pool)
    .await?;
    Ok(Json(json!({ "id": id, "number": number, "total": total })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_transitions() {
        assert!(transition_ok("new", "in_work"));
        assert!(transition_ok("new", "done"));
        assert!(transition_ok("in_work", "ready"));
        assert!(transition_ok("ready", "cancelled"));
        assert!(!transition_ok("ready", "new"));
        assert!(!transition_ok("done", "cancelled"));
        assert!(transition_ok("cancelled", "new"));
        assert!(!transition_ok("cancelled", "done"));
    }
}
