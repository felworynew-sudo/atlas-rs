//! Справочники каталога: /api/product-categories, /api/suppliers, /api/product-units
use crate::{
    auth::AuthContext,
    error::{AppError, AppResult},
    AppState,
};
use axum::{
    extract::{Path, State},
    routing::get,
    Json, Router,
};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, Postgres, Transaction};
use uuid::Uuid;

/// Единицы по умолчанию (Настройки → Единицы).
pub const DEFAULT_UNITS: [(&str, &str); 5] =
    [("Штука", "шт"), ("Килограмм", "кг"), ("Грамм", "г"), ("Литр", "л"), ("Миллилитр", "мл")];

pub async fn seed_default_units(tx: &mut Transaction<'_, Postgres>, org_id: Uuid) -> AppResult<()> {
    for (name, short) in DEFAULT_UNITS {
        // clock_timestamp(), а не now(): порядок единиц внутри одной транзакции сохраняется
        sqlx::query("INSERT INTO product_units (id, organization_id, name, short_name, created_at) VALUES ($1,$2,$3,$4,clock_timestamp())")
            .bind(Uuid::new_v4())
            .bind(org_id)
            .bind(name)
            .bind(short)
            .execute(&mut **tx)
            .await?;
    }
    Ok(())
}

fn need_name(name: &str) -> AppResult<String> {
    let n = name.trim();
    if n.is_empty() {
        return Err(AppError::BadRequest("Введите название".into()));
    }
    Ok(n.to_string())
}

fn ok() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "success": true }))
}

// ---------- категории ----------
pub fn categories_router() -> Router<AppState> {
    Router::new().route("/", get(cat_list).post(cat_create)).route("/{id}", get(cat_one).put(cat_update).delete(cat_remove))
}

#[derive(Serialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct CategoryView {
    pub id: Uuid,
    pub name: String,
    pub sort_order: i32,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CategoryDto {
    pub name: String,
    pub sort_order: Option<i32>,
}

async fn cat_list(State(s): State<AppState>, ctx: AuthContext) -> AppResult<Json<Vec<CategoryView>>> {
    let rows = sqlx::query_as::<_, CategoryView>(
        "SELECT id, name, sort_order FROM product_categories WHERE organization_id=$1 AND deleted_at IS NULL ORDER BY sort_order, name",
    )
    .bind(ctx.org_id)
    .fetch_all(&s.pool)
    .await?;
    Ok(Json(rows))
}

async fn cat_one(State(s): State<AppState>, ctx: AuthContext, Path(id): Path<Uuid>) -> AppResult<Json<CategoryView>> {
    let row = sqlx::query_as::<_, CategoryView>(
        "SELECT id, name, sort_order FROM product_categories WHERE id=$1 AND organization_id=$2 AND deleted_at IS NULL",
    )
    .bind(id)
    .bind(ctx.org_id)
    .fetch_optional(&s.pool)
    .await?
    .ok_or_else(|| AppError::NotFound("Категория не найдена".into()))?;
    Ok(Json(row))
}

async fn cat_create(State(s): State<AppState>, ctx: AuthContext, Json(dto): Json<CategoryDto>) -> AppResult<Json<CategoryView>> {
    ctx.require("settings.categories")?;
    let name = need_name(&dto.name)?;
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO product_categories (id, organization_id, name, sort_order) VALUES ($1,$2,$3,\
         COALESCE($4, (SELECT COALESCE(max(sort_order),0)+1 FROM product_categories WHERE organization_id=$2)))",
    )
    .bind(id)
    .bind(ctx.org_id)
    .bind(name)
    .bind(dto.sort_order)
    .execute(&s.pool)
    .await?;
    cat_one(State(s), ctx, Path(id)).await
}

async fn cat_update(
    State(s): State<AppState>,
    ctx: AuthContext,
    Path(id): Path<Uuid>,
    Json(dto): Json<CategoryDto>,
) -> AppResult<Json<CategoryView>> {
    ctx.require("settings.categories")?;
    let name = need_name(&dto.name)?;
    let res = sqlx::query(
        "UPDATE product_categories SET name=$3, sort_order=COALESCE($4, sort_order) WHERE id=$1 AND organization_id=$2 AND deleted_at IS NULL",
    )
    .bind(id)
    .bind(ctx.org_id)
    .bind(name)
    .bind(dto.sort_order)
    .execute(&s.pool)
    .await?;
    if res.rows_affected() == 0 {
        return Err(AppError::NotFound("Категория не найдена".into()));
    }
    cat_one(State(s), ctx, Path(id)).await
}

async fn cat_remove(State(s): State<AppState>, ctx: AuthContext, Path(id): Path<Uuid>) -> AppResult<Json<serde_json::Value>> {
    ctx.require("settings.categories")?;
    let mut tx = s.pool.begin().await?;
    let res = sqlx::query("UPDATE product_categories SET deleted_at=now() WHERE id=$1 AND organization_id=$2 AND deleted_at IS NULL")
        .bind(id)
        .bind(ctx.org_id)
        .execute(&mut *tx)
        .await?;
    if res.rows_affected() == 0 {
        return Err(AppError::NotFound("Категория не найдена".into()));
    }
    sqlx::query("UPDATE products SET category_id=NULL WHERE category_id=$1").bind(id).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(ok())
}

// ---------- поставщики ----------
pub fn suppliers_router() -> Router<AppState> {
    Router::new().route("/", get(sup_list).post(sup_create)).route("/{id}", get(sup_one).put(sup_update).delete(sup_remove))
}

#[derive(Serialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct SupplierView {
    pub id: Uuid,
    pub name: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SupplierDto {
    pub name: String,
}

async fn sup_list(State(s): State<AppState>, ctx: AuthContext) -> AppResult<Json<Vec<SupplierView>>> {
    let rows = sqlx::query_as::<_, SupplierView>(
        "SELECT id, name FROM suppliers WHERE organization_id=$1 AND deleted_at IS NULL ORDER BY name",
    )
    .bind(ctx.org_id)
    .fetch_all(&s.pool)
    .await?;
    Ok(Json(rows))
}

async fn sup_one(State(s): State<AppState>, ctx: AuthContext, Path(id): Path<Uuid>) -> AppResult<Json<SupplierView>> {
    let row = sqlx::query_as::<_, SupplierView>("SELECT id, name FROM suppliers WHERE id=$1 AND organization_id=$2 AND deleted_at IS NULL")
        .bind(id)
        .bind(ctx.org_id)
        .fetch_optional(&s.pool)
        .await?
        .ok_or_else(|| AppError::NotFound("Поставщик не найден".into()))?;
    Ok(Json(row))
}

async fn sup_create(State(s): State<AppState>, ctx: AuthContext, Json(dto): Json<SupplierDto>) -> AppResult<Json<SupplierView>> {
    ctx.require("settings.suppliers")?;
    let name = need_name(&dto.name)?;
    let id = Uuid::new_v4();
    sqlx::query("INSERT INTO suppliers (id, organization_id, name) VALUES ($1,$2,$3)")
        .bind(id)
        .bind(ctx.org_id)
        .bind(name)
        .execute(&s.pool)
        .await?;
    sup_one(State(s), ctx, Path(id)).await
}

async fn sup_update(
    State(s): State<AppState>,
    ctx: AuthContext,
    Path(id): Path<Uuid>,
    Json(dto): Json<SupplierDto>,
) -> AppResult<Json<SupplierView>> {
    ctx.require("settings.suppliers")?;
    let name = need_name(&dto.name)?;
    let res = sqlx::query("UPDATE suppliers SET name=$3 WHERE id=$1 AND organization_id=$2 AND deleted_at IS NULL")
        .bind(id)
        .bind(ctx.org_id)
        .bind(name)
        .execute(&s.pool)
        .await?;
    if res.rows_affected() == 0 {
        return Err(AppError::NotFound("Поставщик не найден".into()));
    }
    sup_one(State(s), ctx, Path(id)).await
}

async fn sup_remove(State(s): State<AppState>, ctx: AuthContext, Path(id): Path<Uuid>) -> AppResult<Json<serde_json::Value>> {
    ctx.require("settings.suppliers")?;
    let mut tx = s.pool.begin().await?;
    let res = sqlx::query("UPDATE suppliers SET deleted_at=now() WHERE id=$1 AND organization_id=$2 AND deleted_at IS NULL")
        .bind(id)
        .bind(ctx.org_id)
        .execute(&mut *tx)
        .await?;
    if res.rows_affected() == 0 {
        return Err(AppError::NotFound("Поставщик не найден".into()));
    }
    sqlx::query("UPDATE products SET supplier_id=NULL WHERE supplier_id=$1").bind(id).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(ok())
}

// ---------- единицы измерения ----------
pub fn units_router() -> Router<AppState> {
    Router::new().route("/", get(unit_list).post(unit_create)).route("/{id}", get(unit_one).put(unit_update).delete(unit_remove))
}

#[derive(Serialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct UnitView {
    pub id: Uuid,
    pub name: String,
    pub short_name: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UnitDto {
    pub name: String,
    pub short_name: Option<String>,
}

async fn unit_list(State(s): State<AppState>, ctx: AuthContext) -> AppResult<Json<Vec<UnitView>>> {
    let rows = sqlx::query_as::<_, UnitView>(
        "SELECT id, name, short_name FROM product_units WHERE organization_id=$1 AND deleted_at IS NULL ORDER BY created_at, name",
    )
    .bind(ctx.org_id)
    .fetch_all(&s.pool)
    .await?;
    Ok(Json(rows))
}

async fn unit_one(State(s): State<AppState>, ctx: AuthContext, Path(id): Path<Uuid>) -> AppResult<Json<UnitView>> {
    let row = sqlx::query_as::<_, UnitView>(
        "SELECT id, name, short_name FROM product_units WHERE id=$1 AND organization_id=$2 AND deleted_at IS NULL",
    )
    .bind(id)
    .bind(ctx.org_id)
    .fetch_optional(&s.pool)
    .await?
    .ok_or_else(|| AppError::NotFound("Единица измерения не найдена".into()))?;
    Ok(Json(row))
}

async fn unit_create(State(s): State<AppState>, ctx: AuthContext, Json(dto): Json<UnitDto>) -> AppResult<Json<UnitView>> {
    ctx.require("settings.edit")?;
    let name = need_name(&dto.name)?;
    let id = Uuid::new_v4();
    sqlx::query("INSERT INTO product_units (id, organization_id, name, short_name) VALUES ($1,$2,$3,$4)")
        .bind(id)
        .bind(ctx.org_id)
        .bind(name)
        .bind(dto.short_name.unwrap_or_default().trim())
        .execute(&s.pool)
        .await?;
    unit_one(State(s), ctx, Path(id)).await
}

async fn unit_update(
    State(s): State<AppState>,
    ctx: AuthContext,
    Path(id): Path<Uuid>,
    Json(dto): Json<UnitDto>,
) -> AppResult<Json<UnitView>> {
    ctx.require("settings.edit")?;
    let name = need_name(&dto.name)?;
    let res = sqlx::query(
        "UPDATE product_units SET name=$3, short_name=COALESCE($4, short_name) WHERE id=$1 AND organization_id=$2 AND deleted_at IS NULL",
    )
    .bind(id)
    .bind(ctx.org_id)
    .bind(name)
    .bind(dto.short_name.map(|v| v.trim().to_string()))
    .execute(&s.pool)
    .await?;
    if res.rows_affected() == 0 {
        return Err(AppError::NotFound("Единица измерения не найдена".into()));
    }
    unit_one(State(s), ctx, Path(id)).await
}

async fn unit_remove(State(s): State<AppState>, ctx: AuthContext, Path(id): Path<Uuid>) -> AppResult<Json<serde_json::Value>> {
    ctx.require("settings.edit")?;
    let (used,): (i64,) = sqlx::query_as("SELECT count(*) FROM products WHERE unit_id=$1 AND deleted_at IS NULL")
        .bind(id)
        .fetch_one(&s.pool)
        .await?;
    if used > 0 {
        return Err(AppError::BadRequest("Единица используется в товарах".into()));
    }
    let res = sqlx::query("UPDATE product_units SET deleted_at=now() WHERE id=$1 AND organization_id=$2 AND deleted_at IS NULL")
        .bind(id)
        .bind(ctx.org_id)
        .execute(&s.pool)
        .await?;
    if res.rows_affected() == 0 {
        return Err(AppError::NotFound("Единица измерения не найдена".into()));
    }
    Ok(ok())
}
