//! Платежи (/api/payments), обновления (/api/updates), печать этикеток (/api/label-settings, /api/labels/render)
use crate::{
    auth::AuthContext,
    error::{AppError, AppResult},
    routes::{finance::{add_tx, check_wallet, reverse_tx}, sales::check_store},
    AppState,
};
use axum::{
    extract::{Path, Query, State},
    routing::{get, post},
    Json, Router,
};
use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use sqlx::FromRow;
use std::collections::BTreeMap;
use uuid::Uuid;

pub fn payments_router() -> Router<AppState> {
    Router::new().route("/", get(pay_list).post(pay_create)).route("/report", get(pay_report)).route("/{id}", axum::routing::delete(pay_remove))
}

pub fn updates_router() -> Router<AppState> {
    Router::new().route("/", get(upd_list)).route("/read", post(upd_read)).route("/unread-count", get(upd_unread))
}

pub fn labels_router() -> Router<AppState> {
    Router::new().route("/api/label-settings", get(label_get).put(label_put)).route("/api/labels/render", post(label_render))
}

fn ok() -> Json<Value> {
    Json(json!({ "success": true }))
}

// =========================== платежи ===========================
#[derive(Serialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct PaymentView {
    pub id: Uuid,
    pub store_id: Uuid,
    pub user_id: Uuid,
    pub category_id: Option<Uuid>,
    pub category_name: Option<String>,
    pub wallet_id: Uuid,
    pub amount: f64,
    pub paid_on: NaiveDate,
    pub period_from: Option<NaiveDate>,
    pub period_to: Option<NaiveDate>,
    pub details: String,
    pub created_at: DateTime<Utc>,
}

const SELECT_PAYMENTS: &str = "SELECT p.id, p.store_id, p.user_id, p.category_id, c.name AS category_name, p.wallet_id, p.amount::float8 AS amount, \
     p.paid_on, p.period_from, p.period_to, p.details, p.created_at FROM money_payments p LEFT JOIN list_items c ON c.id = p.category_id \
     WHERE p.organization_id=$1 AND p.deleted_at IS NULL AND ($2::uuid IS NULL OR p.id=$2) AND ($3::uuid IS NULL OR p.store_id=$3) \
       AND ( ($6 = 'period' AND ($4::date IS NULL OR COALESCE(p.period_to, p.paid_on) >= $4) AND ($5::date IS NULL OR COALESCE(p.period_from, p.paid_on) <= $5)) \
          OR ($6 <> 'period' AND ($4::date IS NULL OR p.created_at::date >= $4) AND ($5::date IS NULL OR p.created_at::date <= $5)) ) \
     ORDER BY p.paid_on DESC, p.created_at DESC LIMIT 500";

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PayQuery {
    pub store_id: Option<Uuid>,
    pub from: Option<NaiveDate>,
    pub to: Option<NaiveDate>,
    /// created (по дате создания, по умолчанию) | period (по периоду оплаты)
    pub by: Option<String>,
}

async fn pay_fetch(s: &AppState, org: Uuid, id: Option<Uuid>, q: &PayQuery) -> AppResult<Vec<PaymentView>> {
    Ok(sqlx::query_as::<_, PaymentView>(SELECT_PAYMENTS)
        .bind(org)
        .bind(id)
        .bind(q.store_id)
        .bind(q.from)
        .bind(q.to)
        .bind(q.by.as_deref().unwrap_or("created"))
        .fetch_all(&s.pool)
        .await?)
}

async fn pay_list(State(s): State<AppState>, ctx: AuthContext, Query(q): Query<PayQuery>) -> AppResult<Json<Vec<PaymentView>>> {
    ctx.require("payments.view")?;
    Ok(Json(pay_fetch(&s, ctx.org_id, None, &q).await?))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PayDto {
    pub store_id: Uuid,
    pub category_id: Option<Uuid>,
    pub wallet_id: Uuid,
    pub amount: f64,
    pub paid_on: Option<NaiveDate>,
    pub period_from: Option<NaiveDate>,
    pub period_to: Option<NaiveDate>,
    pub details: Option<String>,
}

async fn pay_create(State(s): State<AppState>, ctx: AuthContext, Json(dto): Json<PayDto>) -> AppResult<Json<PaymentView>> {
    ctx.require("payments.view")?;
    check_store(&s.pool, &ctx, dto.store_id).await?;
    if !(dto.amount.is_finite() && dto.amount > 0.0) {
        return Err(AppError::BadRequest("Сумма должна быть больше нуля".into()));
    }
    if let (Some(a), Some(b)) = (dto.period_from, dto.period_to) {
        if b < a {
            return Err(AppError::BadRequest("Конец периода раньше начала".into()));
        }
    }
    check_wallet(&s.pool, ctx.org_id, dto.wallet_id, dto.store_id).await?;
    if let Some(c) = dto.category_id {
        let (n,): (i64,) = sqlx::query_as(
            "SELECT count(*) FROM list_items WHERE id=$1 AND organization_id=$2 AND kind='spending_categories' AND deleted_at IS NULL",
        )
        .bind(c)
        .bind(ctx.org_id)
        .fetch_one(&s.pool)
        .await?;
        if n == 0 {
            return Err(AppError::BadRequest("Категория не найдена".into()));
        }
    }
    let id = Uuid::new_v4();
    let mut tx = s.pool.begin().await?;
    sqlx::query(
        "INSERT INTO money_payments (id, organization_id, store_id, user_id, category_id, wallet_id, amount, paid_on, period_from, period_to, details) \
         VALUES ($1,$2,$3,$4,$5,$6,$7::float8,COALESCE($8, current_date),$9,$10,$11)",
    )
    .bind(id)
    .bind(ctx.org_id)
    .bind(dto.store_id)
    .bind(ctx.user_id)
    .bind(dto.category_id)
    .bind(dto.wallet_id)
    .bind(dto.amount)
    .bind(dto.paid_on)
    .bind(dto.period_from)
    .bind(dto.period_to)
    .bind(dto.details.unwrap_or_default())
    .execute(&mut *tx)
    .await?;
    add_tx(&mut tx, ctx.org_id, dto.wallet_id, -dto.amount, "payment", Some(id), "", ctx.user_id).await?;
    tx.commit().await?;
    Ok(Json(pay_fetch(&s, ctx.org_id, Some(id), &PayQuery::default()).await?.remove(0)))
}

async fn pay_remove(State(s): State<AppState>, ctx: AuthContext, Path(id): Path<Uuid>) -> AppResult<Json<Value>> {
    ctx.require("payments.view")?;
    let mut tx = s.pool.begin().await?;
    let res = sqlx::query("UPDATE money_payments SET deleted_at=now() WHERE id=$1 AND organization_id=$2 AND deleted_at IS NULL")
        .bind(id)
        .bind(ctx.org_id)
        .execute(&mut *tx)
        .await?;
    if res.rows_affected() == 0 {
        return Err(AppError::NotFound("Платёж не найден".into()));
    }
    reverse_tx(&mut tx, id, "payment", ctx.user_id).await?;
    tx.commit().await?;
    Ok(ok())
}

#[derive(Deserialize)]
pub struct ReportQuery {
    pub from: DateTime<Utc>,
    pub to: DateTime<Utc>,
}

/// «Отчёт о движении денег»: по каждому счёту остаток на начало, поступления, выплаты по видам и остаток на конец.
async fn pay_report(State(s): State<AppState>, ctx: AuthContext, Query(q): Query<ReportQuery>) -> AppResult<Json<Value>> {
    ctx.require("payments.view")?;
    if q.from >= q.to {
        return Err(AppError::BadRequest("Начало периода должно быть раньше конца".into()));
    }
    let wallets: Vec<(Uuid, String)> =
        sqlx::query_as("SELECT id, name FROM wallets WHERE organization_id=$1 AND deleted_at IS NULL ORDER BY created_at").bind(ctx.org_id).fetch_all(&s.pool).await?;
    let rows: Vec<(Uuid, String, f64, f64)> = sqlx::query_as(
        "SELECT wallet_id, kind, COALESCE(SUM(delta) FILTER (WHERE delta > 0),0)::float8, COALESCE(-SUM(delta) FILTER (WHERE delta < 0),0)::float8 \
         FROM wallet_transactions WHERE organization_id=$1 AND created_at >= $2 AND created_at < $3 GROUP BY wallet_id, kind",
    )
    .bind(ctx.org_id)
    .bind(q.from)
    .bind(q.to)
    .fetch_all(&s.pool)
    .await?;
    let opening: Vec<(Uuid, f64)> = sqlx::query_as(
        "SELECT wallet_id, COALESCE(SUM(delta),0)::float8 FROM wallet_transactions WHERE organization_id=$1 AND created_at < $2 GROUP BY wallet_id",
    )
    .bind(ctx.org_id)
    .bind(q.from)
    .fetch_all(&s.pool)
    .await?;
    let r2 = atlas_domain::sale::round2;
    let mut out = Vec::new();
    for (id, name) in wallets {
        let open = opening.iter().find(|o| o.0 == id).map_or(0.0, |o| o.1);
        let mut inflow = 0.0;
        let mut outflow = 0.0;
        let mut by_kind: BTreeMap<String, Value> = BTreeMap::new();
        for (_, kind, plus, minus) in rows.iter().filter(|r| r.0 == id) {
            inflow += plus;
            outflow += minus;
            by_kind.insert(kind.clone(), json!({"in": r2(*plus), "out": r2(*minus)}));
        }
        out.push(json!({
            "walletId": id, "name": name, "opening": r2(open), "inflow": r2(inflow), "outflow": r2(outflow),
            "closing": r2(open + inflow - outflow), "byKind": by_kind,
        }));
    }
    Ok(Json(json!({ "wallets": out })))
}

// =========================== обновления ===========================
#[derive(Serialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct EventView {
    pub id: Uuid,
    pub store_id: Option<Uuid>,
    pub user_id: Option<Uuid>,
    pub author: Option<String>,
    pub kind: String,
    pub text: String,
    pub created_at: DateTime<Utc>,
    pub unread: bool,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct UpdQuery {
    pub store_id: Option<Uuid>,
    pub kind: Option<String>,
    pub from: Option<DateTime<Utc>>,
    pub to: Option<DateTime<Utc>>,
}

async fn upd_list(State(s): State<AppState>, ctx: AuthContext, Query(q): Query<UpdQuery>) -> AppResult<Json<Vec<EventView>>> {
    ctx.require("updates.view")?;
    Ok(Json(
        sqlx::query_as::<_, EventView>(
            "SELECT e.id, e.store_id, e.user_id, u.name AS author, e.kind, e.text, e.created_at, \
                    e.created_at > COALESCE((SELECT read_at FROM update_reads WHERE user_id=$2), 'epoch'::timestamptz) AS unread \
             FROM update_events e LEFT JOIN users u ON u.id = e.user_id \
             WHERE e.organization_id=$1 AND ($3::uuid IS NULL OR e.store_id=$3) AND ($4::text IS NULL OR e.kind=$4) \
               AND ($5::timestamptz IS NULL OR e.created_at >= $5) AND ($6::timestamptz IS NULL OR e.created_at < $6) \
             ORDER BY e.created_at DESC LIMIT 500",
        )
        .bind(ctx.org_id)
        .bind(ctx.user_id)
        .bind(q.store_id)
        .bind(q.kind)
        .bind(q.from)
        .bind(q.to)
        .fetch_all(&s.pool)
        .await?,
    ))
}

async fn upd_unread(State(s): State<AppState>, ctx: AuthContext) -> AppResult<Json<Value>> {
    ctx.require("updates.view")?;
    let (n,): (i64,) = sqlx::query_as(
        "SELECT count(*) FROM update_events WHERE organization_id=$1 AND created_at > COALESCE((SELECT read_at FROM update_reads WHERE user_id=$2), 'epoch'::timestamptz)",
    )
    .bind(ctx.org_id)
    .bind(ctx.user_id)
    .fetch_one(&s.pool)
    .await?;
    Ok(Json(json!({ "count": n })))
}

async fn upd_read(State(s): State<AppState>, ctx: AuthContext) -> AppResult<Json<Value>> {
    ctx.require("updates.view")?;
    sqlx::query("INSERT INTO update_reads (user_id, read_at) VALUES ($1, clock_timestamp()) ON CONFLICT (user_id) DO UPDATE SET read_at = clock_timestamp()")
        .bind(ctx.user_id)
        .execute(&s.pool)
        .await?;
    Ok(ok())
}

// =========================== этикетки ===========================
async fn label_get(State(s): State<AppState>, ctx: AuthContext) -> AppResult<Json<Value>> {
    ctx.require("labels.print")?;
    let (v,): (Value,) = sqlx::query_as("SELECT label_settings FROM organizations WHERE id=$1").bind(ctx.org_id).fetch_one(&s.pool).await?;
    Ok(Json(v))
}

/// «Настройки этикетки»: высота, размеры шрифтов, печать даты.
async fn label_put(State(s): State<AppState>, ctx: AuthContext, Json(body): Json<Value>) -> AppResult<Json<Value>> {
    ctx.require("labels.print")?;
    let obj = body.as_object().ok_or_else(|| AppError::BadRequest("Ожидается объект настроек".into()))?;
    let (cur,): (Value,) = sqlx::query_as("SELECT label_settings FROM organizations WHERE id=$1").bind(ctx.org_id).fetch_one(&s.pool).await?;
    let mut next: Map<String, Value> = cur.as_object().cloned().unwrap_or_default();
    for (k, v) in obj {
        let ok = match k.as_str() {
            "labelHeight" => v.as_f64().is_some_and(|n| (20.0..=400.0).contains(&n)),
            "fontSize" | "priceFontSize" => v.as_f64().is_some_and(|n| (4.0..=60.0).contains(&n)),
            "printDate" => v.is_boolean(),
            _ => return Err(AppError::BadRequest(format!("Неизвестная настройка «{k}»"))),
        };
        if !ok {
            return Err(AppError::BadRequest(format!("Некорректное значение настройки «{k}»")));
        }
        next.insert(k.clone(), v.clone());
    }
    let next = Value::Object(next);
    sqlx::query("UPDATE organizations SET label_settings=$2 WHERE id=$1").bind(ctx.org_id).bind(&next).execute(&s.pool).await?;
    Ok(Json(next))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LabelItem {
    pub product_id: Uuid,
    pub copies: u32,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenderDto {
    pub store_id: Uuid,
    pub items: Vec<LabelItem>,
    /// «Дата на этикетке»
    pub date: Option<NaiveDate>,
}

/// Данные для печати: название, цена филиала, штрихкод, число копий и настройки макета (печать — на стороне браузера).
async fn label_render(State(s): State<AppState>, ctx: AuthContext, Json(dto): Json<RenderDto>) -> AppResult<Json<Value>> {
    ctx.require("labels.print")?;
    check_store(&s.pool, &ctx, dto.store_id).await?;
    if dto.items.is_empty() || dto.items.iter().any(|i| i.copies == 0 || i.copies > 1000) {
        return Err(AppError::BadRequest("Выберите товары и число этикеток (1–1000)".into()));
    }
    let ids: Vec<Uuid> = dto.items.iter().map(|i| i.product_id).collect();
    let rows: Vec<(Uuid, String, Vec<String>, f64)> = sqlx::query_as(
        "SELECT p.id, p.name, p.barcodes, COALESCE(ps.sale_price,0)::float8 FROM products p \
         LEFT JOIN product_stores ps ON ps.product_id=p.id AND ps.store_id=$3 WHERE p.organization_id=$1 AND p.deleted_at IS NULL AND p.id = ANY($2)",
    )
    .bind(ctx.org_id)
    .bind(&ids)
    .bind(dto.store_id)
    .fetch_all(&s.pool)
    .await?;
    let (settings,): (Value,) = sqlx::query_as("SELECT label_settings FROM organizations WHERE id=$1").bind(ctx.org_id).fetch_one(&s.pool).await?;
    let print_date = settings.get("printDate").and_then(Value::as_bool).unwrap_or(true);
    let date = dto.date.unwrap_or_else(|| Utc::now().date_naive());
    let mut labels = Vec::new();
    for it in &dto.items {
        let r = rows.iter().find(|r| r.0 == it.product_id).ok_or_else(|| AppError::BadRequest("Товар не найден".into()))?;
        labels.push(json!({
            "productId": r.0, "name": r.1, "price": r.3, "barcode": r.2.first(), "copies": it.copies,
            "date": if print_date { json!(date) } else { Value::Null },
        }));
    }
    Ok(Json(json!({ "settings": settings, "labels": labels })))
}
