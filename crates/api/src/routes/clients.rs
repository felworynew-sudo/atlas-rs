//! /api/clients — клиенты, бонусы, карта «N-й в подарок», статистика; /api/saved-carts — отложенные чеки
use crate::{
    auth::AuthContext,
    error::{AppError, AppResult},
    AppState,
};
use axum::{
    extract::{Path, Query, State},
    routing::{delete, get, post},
    Json, Router,
};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::FromRow;
use uuid::Uuid;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list).post(create))
        .route("/stats", get(stats))
        .route("/{id}", get(one).put(update).delete(remove))
        .route("/{id}/bonus-adjustments", post(bonus_adjust))
        .route("/{id}/punch-card", get(punch_card))
        .route("/{id}/punch-card/reset", post(punch_reset))
        .route("/{id}/purchase-history", get(purchase_history))
}

pub fn saved_carts_router() -> Router<AppState> {
    Router::new().route("/", get(carts_list).post(carts_create)).route("/{id}", delete(carts_remove))
}

#[derive(Serialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct ClientView {
    pub id: Uuid,
    pub name: String,
    pub phone: String,
    pub address: String,
    pub client_type: String,
    pub discount_id: Option<Uuid>,
    pub preferred_price_type_id: Option<Uuid>,
    pub comment: String,
    pub bonus_balance: f64,
    pub bonus_level: Option<f64>,
    pub punches: i32,
    pub purchases: i64,
    pub purchases_sum: f64,
    pub last_purchase_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

const SELECT_CLIENTS: &str = "SELECT c.id, c.name, c.phone, c.address, c.client_type, c.discount_id, c.preferred_price_type_id, c.comment, \
     c.bonus_balance::float8 AS bonus_balance, c.bonus_level::float8 AS bonus_level, c.punch_punches AS punches, \
     (SELECT count(*) FROM sales s WHERE s.client_id = c.id AND s.deleted_at IS NULL) AS purchases, \
     (SELECT COALESCE(SUM(s.total - s.returned_amount),0) FROM sales s WHERE s.client_id = c.id AND s.deleted_at IS NULL)::float8 AS purchases_sum, \
     (SELECT MAX(s.sale_date) FROM sales s WHERE s.client_id = c.id AND s.deleted_at IS NULL) AS last_purchase_at, c.created_at \
     FROM clients c WHERE c.organization_id=$1 AND c.deleted_at IS NULL AND ($2::uuid IS NULL OR c.id=$2) \
       AND ($3::text IS NULL OR c.name ILIKE '%' || $3 || '%' OR c.phone LIKE '%' || $3 || '%') \
       AND ($4::uuid IS NULL OR EXISTS (SELECT 1 FROM sales s JOIN sale_items i ON i.sale_id = s.id \
            WHERE s.client_id = c.id AND s.deleted_at IS NULL AND i.product_id = $4)) \
     ORDER BY CASE WHEN $5 = 'last_purchase' THEN (SELECT MAX(s.sale_date) FROM sales s WHERE s.client_id = c.id) END DESC NULLS LAST, \
              CASE WHEN $5 = 'registered' THEN c.created_at END DESC, c.name LIMIT 500";

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientDto {
    pub name: String,
    pub phone: Option<String>,
    pub address: Option<String>,
    pub client_type: Option<String>,
    pub discount_id: Option<Uuid>,
    pub comment: Option<String>,
    pub bonus_level: Option<f64>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListQuery {
    pub search: Option<String>,
    pub product_id: Option<Uuid>,
    /// name (по умолчанию) | registered | last_purchase
    pub sort: Option<String>,
}

/// Телефон: цифры и обязательный ведущий «+» (проверка подтверждена в Атласе).
fn normalize_phone(p: &str) -> AppResult<String> {
    let t = p.trim();
    if t.is_empty() {
        return Ok(String::new());
    }
    if !t.starts_with('+') {
        return Err(AppError::BadRequest("Номер телефона должен начинаться на +".into()));
    }
    let digits: String = t.chars().filter(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() {
        return Err(AppError::BadRequest("Некорректный номер телефона".into()));
    }
    Ok(format!("+{digits}"))
}

/// «+7•••••••1234»: ролям без права «Карточка клиента» телефон показывается маской.
fn mask_phone(p: &str) -> String {
    let n = p.chars().count();
    if n <= 6 {
        return p.to_string();
    }
    let head: String = p.chars().take(2).collect();
    let tail: String = p.chars().skip(n - 4).collect();
    format!("{head}{}{tail}", "•".repeat(n - 6))
}

fn check(dto: &ClientDto) -> AppResult<()> {
    if dto.name.trim().is_empty() {
        return Err(AppError::BadRequest("Введите имя".into()));
    }
    if let Some(l) = dto.bonus_level {
        if !(0.0..=100.0).contains(&l) {
            return Err(AppError::BadRequest("Уровень кэшбэка от 0 до 100%".into()));
        }
    }
    if let Some(t) = &dto.client_type {
        if !["person", "organization"].contains(&t.as_str()) {
            return Err(AppError::BadRequest("Тип клиента: person или organization".into()));
        }
    }
    Ok(())
}

fn conflict(e: sqlx::Error) -> AppError {
    match AppError::from(e) {
        AppError::Conflict(_) => AppError::Conflict("Клиент с таким телефоном уже есть".into()),
        other => other,
    }
}

async fn check_discount(s: &AppState, ctx: &AuthContext, id: Option<Uuid>) -> AppResult<()> {
    if let Some(d) = id {
        let (n,): (i64,) = sqlx::query_as("SELECT count(*) FROM list_items WHERE id=$1 AND organization_id=$2 AND kind='discounts' AND deleted_at IS NULL")
            .bind(d)
            .bind(ctx.org_id)
            .fetch_one(&s.pool)
            .await?;
        if n == 0 {
            return Err(AppError::BadRequest("Скидка не найдена".into()));
        }
    }
    Ok(())
}

async fn fetch(s: &AppState, ctx: &AuthContext, id: Option<Uuid>, q: &ListQuery) -> AppResult<Vec<ClientView>> {
    let search = q.search.as_deref().map(str::trim).filter(|v| !v.is_empty());
    let mut rows = sqlx::query_as::<_, ClientView>(SELECT_CLIENTS)
        .bind(ctx.org_id)
        .bind(id)
        .bind(search)
        .bind(q.product_id)
        .bind(q.sort.as_deref().unwrap_or("name"))
        .fetch_all(&s.pool)
        .await?;
    if !ctx.can("client.card") {
        for r in &mut rows {
            r.phone = mask_phone(&r.phone);
        }
    }
    Ok(rows)
}

async fn list(State(s): State<AppState>, ctx: AuthContext, Query(q): Query<ListQuery>) -> AppResult<Json<Vec<ClientView>>> {
    ctx.require("client.view")?;
    Ok(Json(fetch(&s, &ctx, None, &q).await?))
}

async fn one(State(s): State<AppState>, ctx: AuthContext, Path(id): Path<Uuid>) -> AppResult<Json<ClientView>> {
    ctx.require("client.card")?;
    fetch(&s, &ctx, Some(id), &ListQuery { search: None, product_id: None, sort: None })
        .await?
        .into_iter()
        .next()
        .map(Json)
        .ok_or_else(|| AppError::NotFound("Клиент не найден".into()))
}

async fn create(State(s): State<AppState>, ctx: AuthContext, Json(dto): Json<ClientDto>) -> AppResult<Json<ClientView>> {
    ctx.require("client.view")?;
    check(&dto)?;
    check_discount(&s, &ctx, dto.discount_id).await?;
    let phone = normalize_phone(dto.phone.as_deref().unwrap_or(""))?;
    let id = Uuid::new_v4();
    // уровень бонусов нового клиента — «стартовый процент» из настроек лояльности
    sqlx::query(
        "INSERT INTO clients (id, organization_id, name, phone, address, client_type, discount_id, comment, bonus_level) \
         VALUES ($1,$2,$3,$4,$5,COALESCE($6,'person'),$7,$8,\
         COALESCE($9::float8, (SELECT (loyalty->>'defaultBonusLevel')::float8 FROM organizations WHERE id=$2)))",
    )
    .bind(id)
    .bind(ctx.org_id)
    .bind(dto.name.trim())
    .bind(phone)
    .bind(dto.address.unwrap_or_default())
    .bind(&dto.client_type)
    .bind(dto.discount_id)
    .bind(dto.comment.unwrap_or_default())
    .bind(dto.bonus_level)
    .execute(&s.pool)
    .await
    .map_err(conflict)?;
    one(State(s), ctx, Path(id)).await
}

async fn update(
    State(s): State<AppState>,
    ctx: AuthContext,
    Path(id): Path<Uuid>,
    Json(dto): Json<ClientDto>,
) -> AppResult<Json<ClientView>> {
    ctx.require("client.card")?;
    check(&dto)?;
    check_discount(&s, &ctx, dto.discount_id).await?;
    let phone = dto.phone.as_deref().map(normalize_phone).transpose()?;
    let res = sqlx::query(
        "UPDATE clients SET name=$3, phone=COALESCE($4, phone), address=COALESCE($5, address), client_type=COALESCE($6, client_type), \
         discount_id=$7, comment=COALESCE($8, comment), bonus_level=COALESCE($9::float8, bonus_level) \
         WHERE id=$1 AND organization_id=$2 AND deleted_at IS NULL",
    )
    .bind(id)
    .bind(ctx.org_id)
    .bind(dto.name.trim())
    .bind(phone)
    .bind(&dto.address)
    .bind(&dto.client_type)
    .bind(dto.discount_id)
    .bind(&dto.comment)
    .bind(dto.bonus_level)
    .execute(&s.pool)
    .await
    .map_err(conflict)?;
    if res.rows_affected() == 0 {
        return Err(AppError::NotFound("Клиент не найден".into()));
    }
    one(State(s), ctx, Path(id)).await
}

async fn remove(State(s): State<AppState>, ctx: AuthContext, Path(id): Path<Uuid>) -> AppResult<Json<Value>> {
    ctx.require("client.delete")?;
    let res = sqlx::query("UPDATE clients SET deleted_at=now() WHERE id=$1 AND organization_id=$2 AND deleted_at IS NULL")
        .bind(id)
        .bind(ctx.org_id)
        .execute(&s.pool)
        .await?;
    if res.rows_affected() == 0 {
        return Err(AppError::NotFound("Клиент не найден".into()));
    }
    Ok(Json(json!({ "success": true })))
}

// ---------- статистика ----------
#[derive(Deserialize)]
pub struct StatsQuery {
    pub days: Option<i64>,
}

/// «Всего / Новые / Активные клиенты» за период (7 или 30 суток и т.п.).
async fn stats(State(s): State<AppState>, ctx: AuthContext, Query(q): Query<StatsQuery>) -> AppResult<Json<Value>> {
    ctx.require("client.view")?;
    let since = Utc::now() - Duration::days(q.days.unwrap_or(7).clamp(1, 366));
    let (total, new, active): (i64, i64, i64) = sqlx::query_as(
        "SELECT count(*), count(*) FILTER (WHERE created_at >= $2), \
                count(*) FILTER (WHERE EXISTS (SELECT 1 FROM sales s WHERE s.client_id = c.id AND s.deleted_at IS NULL AND s.sale_date >= $2)) \
         FROM clients c WHERE organization_id=$1 AND deleted_at IS NULL",
    )
    .bind(ctx.org_id)
    .bind(since)
    .fetch_one(&s.pool)
    .await?;
    Ok(Json(json!({ "total": total, "new": new, "active": active })))
}

// ---------- бонусы ----------
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BonusDto {
    /// со знаком: начислить > 0, списать < 0
    pub amount: f64,
    pub comment: Option<String>,
    /// ключ идемпотентности: повтор с тем же значением не меняет баланс
    pub operation_id: Option<String>,
}

async fn bonus_adjust(
    State(s): State<AppState>,
    ctx: AuthContext,
    Path(id): Path<Uuid>,
    Json(dto): Json<BonusDto>,
) -> AppResult<Json<Value>> {
    ctx.require("client.card")?;
    if !dto.amount.is_finite() || dto.amount == 0.0 {
        return Err(AppError::BadRequest("Сумма не может быть нулевой".into()));
    }
    let op = dto.operation_id.filter(|o| !o.is_empty()).unwrap_or_else(|| Uuid::new_v4().to_string());
    let mut tx = s.pool.begin().await?;
    let row: Option<(f64,)> = sqlx::query_as(
        "SELECT bonus_balance::float8 FROM clients WHERE id=$1 AND organization_id=$2 AND deleted_at IS NULL FOR UPDATE",
    )
    .bind(id)
    .bind(ctx.org_id)
    .fetch_optional(&mut *tx)
    .await?;
    let (before,) = row.ok_or_else(|| AppError::NotFound("Клиент не найден".into()))?;
    let done: Option<(f64, f64)> =
        sqlx::query_as("SELECT bonus_before::float8, bonus_after::float8 FROM client_bonus_ops WHERE client_id=$1 AND operation_id=$2")
            .bind(id)
            .bind(&op)
            .fetch_optional(&mut *tx)
            .await?;
    if let Some((b, a)) = done {
        return Ok(Json(json!({ "bonusBefore": b, "bonusAfter": a, "duplicate": true })));
    }
    let after = ((before + dto.amount) * 100.0).round() / 100.0;
    if after < 0.0 {
        return Err(AppError::BadRequest("Бонусов не может быть меньше нуля".into()));
    }
    sqlx::query("UPDATE clients SET bonus_balance=$2::float8 WHERE id=$1").bind(id).bind(after).execute(&mut *tx).await?;
    sqlx::query(
        "INSERT INTO client_bonus_ops (id, organization_id, client_id, operation_id, delta, bonus_before, bonus_after, comment, user_id) \
         VALUES ($1,$2,$3,$4,$5::float8,$6::float8,$7::float8,$8,$9)",
    )
    .bind(Uuid::new_v4())
    .bind(ctx.org_id)
    .bind(id)
    .bind(&op)
    .bind(dto.amount)
    .bind(before)
    .bind(after)
    .bind(dto.comment.unwrap_or_default())
    .bind(ctx.user_id)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(Json(json!({ "bonusBefore": before, "bonusAfter": after, "duplicate": false })))
}

// ---------- карта «N-й товар в подарок» ----------
async fn punch_card(State(s): State<AppState>, ctx: AuthContext, Path(id): Path<Uuid>) -> AppResult<Json<Value>> {
    ctx.require("client.card")?;
    let row: Option<(i32, Value)> = sqlx::query_as(
        "SELECT c.punch_punches, o.loyalty FROM clients c JOIN organizations o ON o.id = c.organization_id \
         WHERE c.id=$1 AND c.organization_id=$2 AND c.deleted_at IS NULL",
    )
    .bind(id)
    .bind(ctx.org_id)
    .fetch_optional(&s.pool)
    .await?;
    let (punches, loyalty) = row.ok_or_else(|| AppError::NotFound("Клиент не найден".into()))?;
    Ok(Json(json!({
        "punches": punches,
        "capacity": loyalty.get("punchCardCapacity").and_then(Value::as_i64).unwrap_or(7),
        "enabled": loyalty.get("punchCardEnabled").and_then(Value::as_bool).unwrap_or(false),
    })))
}

async fn punch_reset(State(s): State<AppState>, ctx: AuthContext, Path(id): Path<Uuid>) -> AppResult<Json<Value>> {
    ctx.require("client.card")?;
    let res = sqlx::query("UPDATE clients SET punch_punches=0 WHERE id=$1 AND organization_id=$2 AND deleted_at IS NULL")
        .bind(id)
        .bind(ctx.org_id)
        .execute(&s.pool)
        .await?;
    if res.rows_affected() == 0 {
        return Err(AppError::NotFound("Клиент не найден".into()));
    }
    Ok(Json(json!({ "punches": 0 })))
}

#[derive(Serialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct PurchaseRow {
    pub id: Uuid,
    pub store_id: Uuid,
    pub sale_date: DateTime<Utc>,
    pub total: f64,
    pub returned_amount: f64,
}

async fn purchase_history(State(s): State<AppState>, ctx: AuthContext, Path(id): Path<Uuid>) -> AppResult<Json<Vec<PurchaseRow>>> {
    ctx.require("client.card")?;
    Ok(Json(
        sqlx::query_as::<_, PurchaseRow>(
            "SELECT id, store_id, sale_date, total::float8 AS total, returned_amount::float8 AS returned_amount FROM sales \
             WHERE organization_id=$1 AND client_id=$2 AND deleted_at IS NULL ORDER BY sale_date DESC LIMIT 500",
        )
        .bind(ctx.org_id)
        .bind(id)
        .fetch_all(&s.pool)
        .await?,
    ))
}

// ---------- отложенные чеки ----------
#[derive(Serialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct CartView {
    pub id: Uuid,
    pub store_id: Uuid,
    pub user_id: Uuid,
    pub name: String,
    pub payload: Value,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CartDto {
    pub store_id: Uuid,
    pub name: Option<String>,
    pub payload: Value,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CartQuery {
    pub store_id: Option<Uuid>,
}

async fn carts_list(State(s): State<AppState>, ctx: AuthContext, Query(q): Query<CartQuery>) -> AppResult<Json<Vec<CartView>>> {
    ctx.require("sale.new")?;
    let rows = sqlx::query_as::<_, CartView>(
        "SELECT id, store_id, user_id, name, payload FROM saved_carts \
         WHERE organization_id=$1 AND ($2::uuid IS NULL OR store_id=$2) ORDER BY created_at",
    )
    .bind(ctx.org_id)
    .bind(q.store_id)
    .fetch_all(&s.pool)
    .await?;
    Ok(Json(rows))
}

async fn carts_create(State(s): State<AppState>, ctx: AuthContext, Json(dto): Json<CartDto>) -> AppResult<Json<CartView>> {
    ctx.require("sale.new")?;
    if !dto.payload.is_object() {
        return Err(AppError::BadRequest("Корзина должна быть объектом".into()));
    }
    let (n,): (i64,) = sqlx::query_as("SELECT count(*) FROM stores WHERE id=$1 AND organization_id=$2 AND deleted_at IS NULL")
        .bind(dto.store_id)
        .bind(ctx.org_id)
        .fetch_one(&s.pool)
        .await?;
    if n == 0 {
        return Err(AppError::BadRequest("Филиал не найден".into()));
    }
    let id = Uuid::new_v4();
    let name = dto.name.unwrap_or_default();
    sqlx::query("INSERT INTO saved_carts (id, organization_id, store_id, user_id, name, payload) VALUES ($1,$2,$3,$4,$5,$6)")
        .bind(id)
        .bind(ctx.org_id)
        .bind(dto.store_id)
        .bind(ctx.user_id)
        .bind(&name)
        .bind(&dto.payload)
        .execute(&s.pool)
        .await?;
    Ok(Json(CartView { id, store_id: dto.store_id, user_id: ctx.user_id, name, payload: dto.payload }))
}

async fn carts_remove(State(s): State<AppState>, ctx: AuthContext, Path(id): Path<Uuid>) -> AppResult<Json<Value>> {
    ctx.require("sale.new")?;
    let res = sqlx::query("DELETE FROM saved_carts WHERE id=$1 AND organization_id=$2").bind(id).bind(ctx.org_id).execute(&s.pool).await?;
    if res.rows_affected() == 0 {
        return Err(AppError::NotFound("Чек не найден".into()));
    }
    Ok(Json(json!({ "success": true })))
}
