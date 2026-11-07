//! Ценовые категории: /api/price-types (список, ручные цены, расчёт цен для кассы, предпочитаемая категория клиента).
use crate::{
    auth::AuthContext,
    error::{AppError, AppResult},
    AppState,
};
use axum::{
    extract::{Path, Query, State},
    routing::{get, put},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::FromRow;
use uuid::Uuid;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list).post(create))
        .route("/client/{client_id}", put(set_client))
        .route("/{id}", get(one).put(update).delete(remove))
        .route("/{id}/prices", get(prices_get).put(prices_put))
        .route("/{id}/effective", get(effective))
}

const NIL: Uuid = Uuid::nil();

#[derive(Serialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct PriceTypeView {
    pub id: Uuid,
    pub name: String,
    pub scope: String,
    pub auto_calc: bool,
    pub markup_percent: f64,
    pub rules: Value,
}

const SELECT: &str = "SELECT id, name, scope, auto_calc, markup_percent::float8 AS markup_percent, rules FROM price_types \
     WHERE organization_id=$1 AND deleted_at IS NULL AND ($2::uuid IS NULL OR id=$2) ORDER BY created_at";

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Dto {
    pub name: String,
    pub scope: Option<String>,
    pub auto_calc: Option<bool>,
    pub markup_percent: Option<f64>,
    pub rules: Option<Value>,
}

fn check(dto: &Dto) -> AppResult<()> {
    if dto.name.trim().is_empty() {
        return Err(AppError::BadRequest("Введите название типа цены".into()));
    }
    if let Some(s) = &dto.scope {
        if s != "store" && s != "common" {
            return Err(AppError::BadRequest("Неизвестная область действия".into()));
        }
    }
    if let Some(m) = dto.markup_percent {
        if !(m.is_finite() && (-100.0..=10000.0).contains(&m)) {
            return Err(AppError::BadRequest("Некорректная наценка".into()));
        }
    }
    if let Some(r) = &dto.rules {
        let obj = r.as_object().ok_or_else(|| AppError::BadRequest("Правила должны быть объектом".into()))?;
        for (k, v) in obj {
            let ok = match k.as_str() {
                "minQty" | "minTotal" => v.is_null() || v.as_f64().is_some_and(|n| n >= 0.0),
                "paymentMethodId" => v.is_null() || v.as_str().is_some_and(|s| Uuid::parse_str(s).is_ok()),
                "byClient" => v.is_boolean(),
                _ => false,
            };
            if !ok {
                return Err(AppError::BadRequest(format!("Некорректное правило «{k}»")));
            }
        }
    }
    Ok(())
}

async fn list(State(s): State<AppState>, ctx: AuthContext) -> AppResult<Json<Vec<PriceTypeView>>> {
    Ok(Json(sqlx::query_as(SELECT).bind(ctx.org_id).bind(None::<Uuid>).fetch_all(&s.pool).await?))
}

async fn fetch_one(s: &AppState, org: Uuid, id: Uuid) -> AppResult<PriceTypeView> {
    sqlx::query_as(SELECT).bind(org).bind(Some(id)).fetch_optional(&s.pool).await?.ok_or_else(|| AppError::NotFound("Тип цены не найден".into()))
}

async fn one(State(s): State<AppState>, ctx: AuthContext, Path(id): Path<Uuid>) -> AppResult<Json<PriceTypeView>> {
    Ok(Json(fetch_one(&s, ctx.org_id, id).await?))
}

async fn create(State(s): State<AppState>, ctx: AuthContext, Json(dto): Json<Dto>) -> AppResult<Json<PriceTypeView>> {
    ctx.require("settings.edit")?;
    check(&dto)?;
    let id = Uuid::new_v4();
    sqlx::query("INSERT INTO price_types (id, organization_id, name, scope, auto_calc, markup_percent, rules) VALUES ($1,$2,$3,$4,$5,$6::float8,$7)")
        .bind(id)
        .bind(ctx.org_id)
        .bind(dto.name.trim())
        .bind(dto.scope.as_deref().unwrap_or("store"))
        .bind(dto.auto_calc.unwrap_or(false))
        .bind(dto.markup_percent.unwrap_or(0.0))
        .bind(dto.rules.clone().unwrap_or_else(|| json!({})))
        .execute(&s.pool)
        .await?;
    Ok(Json(fetch_one(&s, ctx.org_id, id).await?))
}

async fn update(State(s): State<AppState>, ctx: AuthContext, Path(id): Path<Uuid>, Json(dto): Json<Dto>) -> AppResult<Json<PriceTypeView>> {
    ctx.require("settings.edit")?;
    check(&dto)?;
    let res = sqlx::query(
        "UPDATE price_types SET name=$3, scope=COALESCE($4, scope), auto_calc=COALESCE($5, auto_calc), markup_percent=COALESCE($6::float8, markup_percent), \
         rules=COALESCE($7, rules) WHERE id=$1 AND organization_id=$2 AND deleted_at IS NULL",
    )
    .bind(id)
    .bind(ctx.org_id)
    .bind(dto.name.trim())
    .bind(dto.scope.as_deref())
    .bind(dto.auto_calc)
    .bind(dto.markup_percent)
    .bind(dto.rules.as_ref())
    .execute(&s.pool)
    .await?;
    if res.rows_affected() == 0 {
        return Err(AppError::NotFound("Тип цены не найден".into()));
    }
    Ok(Json(fetch_one(&s, ctx.org_id, id).await?))
}

async fn remove(State(s): State<AppState>, ctx: AuthContext, Path(id): Path<Uuid>) -> AppResult<Json<Value>> {
    ctx.require("settings.edit")?;
    let mut tx = s.pool.begin().await?;
    let res = sqlx::query("UPDATE price_types SET deleted_at=now() WHERE id=$1 AND organization_id=$2 AND deleted_at IS NULL")
        .bind(id)
        .bind(ctx.org_id)
        .execute(&mut *tx)
        .await?;
    if res.rows_affected() == 0 {
        return Err(AppError::NotFound("Тип цены не найден".into()));
    }
    sqlx::query("UPDATE clients SET preferred_price_type_id=NULL WHERE preferred_price_type_id=$1").bind(id).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(Json(json!({ "success": true })))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoreQ {
    pub store_id: Option<Uuid>,
}

#[derive(Serialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct PriceRow {
    pub product_id: Uuid,
    pub store_id: Uuid,
    pub price: f64,
}

/// Ручные цены категории (для категорий без авто-расчёта).
async fn prices_get(State(s): State<AppState>, ctx: AuthContext, Path(id): Path<Uuid>) -> AppResult<Json<Vec<PriceRow>>> {
    fetch_one(&s, ctx.org_id, id).await?;
    Ok(Json(
        sqlx::query_as("SELECT product_id, store_id, price::float8 AS price FROM product_price_type_prices WHERE price_type_id=$1")
            .bind(id)
            .fetch_all(&s.pool)
            .await?,
    ))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PriceDto {
    pub product_id: Uuid,
    pub store_id: Option<Uuid>,
    /// `null` — убрать ручную цену
    pub price: Option<f64>,
}

async fn prices_put(State(s): State<AppState>, ctx: AuthContext, Path(id): Path<Uuid>, Json(rows): Json<Vec<PriceDto>>) -> AppResult<Json<Value>> {
    ctx.require("product.view")?;
    let pt = fetch_one(&s, ctx.org_id, id).await?;
    let mut tx = s.pool.begin().await?;
    for r in &rows {
        let store = if pt.scope == "common" { NIL } else { r.store_id.ok_or_else(|| AppError::BadRequest("Укажите филиал".into()))? };
        let (n,): (i64,) = sqlx::query_as("SELECT count(*) FROM products WHERE id=$1 AND organization_id=$2 AND deleted_at IS NULL")
            .bind(r.product_id)
            .bind(ctx.org_id)
            .fetch_one(&mut *tx)
            .await?;
        if n == 0 {
            return Err(AppError::BadRequest("Товар не найден".into()));
        }
        match r.price {
            Some(p) if p.is_finite() && p >= 0.0 => {
                sqlx::query(
                    "INSERT INTO product_price_type_prices (price_type_id, product_id, store_id, price) VALUES ($1,$2,$3,$4::float8) \
                     ON CONFLICT (price_type_id, product_id, store_id) DO UPDATE SET price = EXCLUDED.price",
                )
                .bind(id)
                .bind(r.product_id)
                .bind(store)
                .bind(p)
                .execute(&mut *tx)
                .await?;
            }
            Some(_) => return Err(AppError::BadRequest("Цена не может быть отрицательной".into())),
            None => {
                sqlx::query("DELETE FROM product_price_type_prices WHERE price_type_id=$1 AND product_id=$2 AND store_id=$3")
                    .bind(id)
                    .bind(r.product_id)
                    .bind(store)
                    .execute(&mut *tx)
                    .await?;
            }
        }
    }
    tx.commit().await?;
    Ok(Json(json!({ "success": true })))
}

/// Цены всех активных товаров филиала по этой категории: ручная цена, иначе авто-расчёт, иначе базовая. Для экрана «Новая продажа».
async fn effective(State(s): State<AppState>, ctx: AuthContext, Path(id): Path<Uuid>, Query(q): Query<StoreQ>) -> AppResult<Json<Value>> {
    let pt = fetch_one(&s, ctx.org_id, id).await?;
    let store = q.store_id.ok_or_else(|| AppError::BadRequest("Укажите филиал".into()))?;
    let rows: Vec<(Uuid, f64, Option<f64>)> = sqlx::query_as(
        "SELECT p.id, COALESCE(ps.sale_price,0)::float8, m.price::float8 FROM products p \
         LEFT JOIN product_stores ps ON ps.product_id=p.id AND ps.store_id=$3 \
         LEFT JOIN product_price_type_prices m ON m.product_id=p.id AND m.price_type_id=$2 AND m.store_id = CASE WHEN $4 = 'common' THEN '00000000-0000-0000-0000-000000000000'::uuid ELSE $3 END \
         WHERE p.organization_id=$1 AND p.deleted_at IS NULL",
    )
    .bind(ctx.org_id)
    .bind(id)
    .bind(store)
    .bind(&pt.scope)
    .fetch_all(&s.pool)
    .await?;
    let r2 = atlas_domain::sale::round2;
    let mut map = serde_json::Map::new();
    for (pid, base, manual) in rows {
        let price = match manual {
            Some(m) => m,
            None if pt.auto_calc => r2(base * (1.0 + pt.markup_percent / 100.0)),
            None => base,
        };
        map.insert(pid.to_string(), json!(price));
    }
    Ok(Json(Value::Object(map)))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientDto {
    pub price_type_id: Option<Uuid>,
}

/// Предпочитаемая ценовая категория клиента.
async fn set_client(State(s): State<AppState>, ctx: AuthContext, Path(client_id): Path<Uuid>, Json(dto): Json<ClientDto>) -> AppResult<Json<Value>> {
    ctx.require("client.card")?;
    if let Some(pt) = dto.price_type_id {
        fetch_one(&s, ctx.org_id, pt).await?;
    }
    let res = sqlx::query("UPDATE clients SET preferred_price_type_id=$3 WHERE id=$1 AND organization_id=$2 AND deleted_at IS NULL")
        .bind(client_id)
        .bind(ctx.org_id)
        .bind(dto.price_type_id)
        .execute(&s.pool)
        .await?;
    if res.rows_affected() == 0 {
        return Err(AppError::NotFound("Клиент не найден".into()));
    }
    Ok(Json(json!({ "success": true })))
}
