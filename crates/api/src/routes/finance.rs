//! Финансы: /api/wallets (денежные счета), /api/spendings (расходы), /api/recurring-expenses (ежемесячные)
use crate::{
    auth::AuthContext,
    error::{AppError, AppResult},
    routes::sales::check_store,
    AppState,
};
use axum::{
    extract::{Path, Query, State},
    routing::{get, post},
    Json, Router,
};
use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::{FromRow, Postgres, Transaction};
use uuid::Uuid;

pub fn wallets_router() -> Router<AppState> {
    Router::new()
        .route("/", get(wallets_list).post(wallet_create))
        .route("/transfer", post(wallet_transfer))
        .route("/{id}", get(wallet_one).put(wallet_update).delete(wallet_remove))
        .route("/{id}/transactions", get(tx_list).post(tx_manual))
}
pub fn spendings_router() -> Router<AppState> {
    Router::new().route("/", get(spendings_list).post(spending_create)).route("/{id}", get(spending_one).delete(spending_remove))
}
pub fn recurring_router() -> Router<AppState> {
    Router::new().route("/", get(rec_list).post(rec_create)).route("/{id}", get(rec_one).put(rec_update).delete(rec_remove))
}

fn ok() -> Json<Value> {
    Json(json!({ "success": true }))
}

fn money(v: f64) -> bool {
    v.is_finite() && v > 0.0
}

// ---------- проводки по счетам ----------
#[allow(clippy::too_many_arguments)]
pub async fn add_tx(
    tx: &mut Transaction<'_, Postgres>,
    org_id: Uuid,
    wallet_id: Uuid,
    delta: f64,
    kind: &str,
    ref_id: Option<Uuid>,
    comment: &str,
    user_id: Uuid,
) -> AppResult<()> {
    sqlx::query(
        "INSERT INTO wallet_transactions (id, organization_id, wallet_id, delta, kind, ref_id, comment, user_id) \
         VALUES ($1,$2,$3,$4::float8,$5,$6,$7,$8)",
    )
    .bind(Uuid::new_v4())
    .bind(org_id)
    .bind(wallet_id)
    .bind(delta)
    .bind(kind)
    .bind(ref_id)
    .bind(comment)
    .bind(user_id)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// Обратная проводка на чистый эффект документа (расход, оплата приёмки).
pub async fn reverse_tx(tx: &mut Transaction<'_, Postgres>, ref_id: Uuid, kind: &str, user_id: Uuid) -> AppResult<()> {
    sqlx::query(
        "INSERT INTO wallet_transactions (id, organization_id, wallet_id, delta, kind, ref_id, comment, user_id) \
         SELECT gen_random_uuid(), organization_id, wallet_id, -SUM(delta), kind, ref_id, 'Отмена', $3 \
         FROM wallet_transactions WHERE ref_id=$1 AND kind=$2 \
         GROUP BY organization_id, wallet_id, kind, ref_id HAVING SUM(delta) <> 0",
    )
    .bind(ref_id)
    .bind(kind)
    .bind(user_id)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// Счёт принадлежит организации и подходит филиалу (общий или этого филиала).
pub async fn check_wallet(pool: &sqlx::PgPool, org_id: Uuid, wallet_id: Uuid, store_id: Uuid) -> AppResult<()> {
    let (n,): (i64,) = sqlx::query_as(
        "SELECT count(*) FROM wallets WHERE id=$1 AND organization_id=$2 AND deleted_at IS NULL AND (store_id IS NULL OR store_id=$3)",
    )
    .bind(wallet_id)
    .bind(org_id)
    .bind(store_id)
    .fetch_one(pool)
    .await?;
    if n == 0 {
        return Err(AppError::BadRequest("Денежный счёт не найден".into()));
    }
    Ok(())
}

// =========================== счета ===========================
#[derive(Serialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct WalletView {
    pub id: Uuid,
    pub name: String,
    pub store_id: Option<Uuid>,
    pub balance: f64,
}

const SELECT_WALLET: &str = "SELECT w.id, w.name, w.store_id, \
     (SELECT COALESCE(SUM(t.delta),0) FROM wallet_transactions t WHERE t.wallet_id = w.id)::float8 AS balance \
     FROM wallets w WHERE w.organization_id=$1 AND w.deleted_at IS NULL AND ($2::uuid IS NULL OR w.id=$2) ORDER BY w.created_at";

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WalletDto {
    pub name: String,
    pub store_id: Option<Uuid>,
}

async fn wallets_list(State(s): State<AppState>, ctx: AuthContext) -> AppResult<Json<Vec<WalletView>>> {
    Ok(Json(sqlx::query_as::<_, WalletView>(SELECT_WALLET).bind(ctx.org_id).bind(None::<Uuid>).fetch_all(&s.pool).await?))
}

async fn wallet_one(State(s): State<AppState>, ctx: AuthContext, Path(id): Path<Uuid>) -> AppResult<Json<WalletView>> {
    sqlx::query_as::<_, WalletView>(SELECT_WALLET)
        .bind(ctx.org_id)
        .bind(Some(id))
        .fetch_optional(&s.pool)
        .await?
        .map(Json)
        .ok_or_else(|| AppError::NotFound("Счёт не найден".into()))
}

async fn check_wallet_dto(s: &AppState, ctx: &AuthContext, dto: &WalletDto) -> AppResult<()> {
    if dto.name.trim().is_empty() {
        return Err(AppError::BadRequest("Введите название".into()));
    }
    if let Some(st) = dto.store_id {
        let (n,): (i64,) = sqlx::query_as("SELECT count(*) FROM stores WHERE id=$1 AND organization_id=$2 AND deleted_at IS NULL")
            .bind(st)
            .bind(ctx.org_id)
            .fetch_one(&s.pool)
            .await?;
        if n == 0 {
            return Err(AppError::BadRequest("Филиал не найден".into()));
        }
    }
    Ok(())
}

async fn wallet_create(State(s): State<AppState>, ctx: AuthContext, Json(dto): Json<WalletDto>) -> AppResult<Json<WalletView>> {
    ctx.require("settings.edit")?;
    check_wallet_dto(&s, &ctx, &dto).await?;
    let id = Uuid::new_v4();
    sqlx::query("INSERT INTO wallets (id, organization_id, store_id, name) VALUES ($1,$2,$3,$4)")
        .bind(id)
        .bind(ctx.org_id)
        .bind(dto.store_id)
        .bind(dto.name.trim())
        .execute(&s.pool)
        .await?;
    wallet_one(State(s), ctx, Path(id)).await
}

async fn wallet_update(
    State(s): State<AppState>,
    ctx: AuthContext,
    Path(id): Path<Uuid>,
    Json(dto): Json<WalletDto>,
) -> AppResult<Json<WalletView>> {
    ctx.require("settings.edit")?;
    check_wallet_dto(&s, &ctx, &dto).await?;
    let res = sqlx::query("UPDATE wallets SET name=$3, store_id=$4 WHERE id=$1 AND organization_id=$2 AND deleted_at IS NULL")
        .bind(id)
        .bind(ctx.org_id)
        .bind(dto.name.trim())
        .bind(dto.store_id)
        .execute(&s.pool)
        .await?;
    if res.rows_affected() == 0 {
        return Err(AppError::NotFound("Счёт не найден".into()));
    }
    wallet_one(State(s), ctx, Path(id)).await
}

async fn wallet_remove(State(s): State<AppState>, ctx: AuthContext, Path(id): Path<Uuid>) -> AppResult<Json<Value>> {
    ctx.require("settings.edit")?;
    let res = sqlx::query("UPDATE wallets SET deleted_at=now() WHERE id=$1 AND organization_id=$2 AND deleted_at IS NULL")
        .bind(id)
        .bind(ctx.org_id)
        .execute(&s.pool)
        .await?;
    if res.rows_affected() == 0 {
        return Err(AppError::NotFound("Счёт не найден".into()));
    }
    Ok(ok())
}

#[derive(Serialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct TxView {
    pub id: Uuid,
    pub delta: f64,
    pub kind: String,
    pub ref_id: Option<Uuid>,
    pub comment: String,
    pub created_at: DateTime<Utc>,
}

async fn tx_list(State(s): State<AppState>, ctx: AuthContext, Path(id): Path<Uuid>) -> AppResult<Json<Vec<TxView>>> {
    ctx.require("payments.view")?;
    Ok(Json(
        sqlx::query_as::<_, TxView>(
            "SELECT id, delta::float8 AS delta, kind, ref_id, comment, created_at FROM wallet_transactions \
             WHERE organization_id=$1 AND wallet_id=$2 ORDER BY created_at DESC, id LIMIT 500",
        )
        .bind(ctx.org_id)
        .bind(id)
        .fetch_all(&s.pool)
        .await?,
    ))
}

#[derive(Deserialize)]
pub struct ManualTxDto {
    /// со знаком: внесение > 0, изъятие < 0
    pub amount: f64,
    pub comment: Option<String>,
}

async fn tx_manual(
    State(s): State<AppState>,
    ctx: AuthContext,
    Path(id): Path<Uuid>,
    Json(dto): Json<ManualTxDto>,
) -> AppResult<Json<WalletView>> {
    ctx.require("settings.edit")?;
    if !dto.amount.is_finite() || dto.amount == 0.0 {
        return Err(AppError::BadRequest("Укажите сумму".into()));
    }
    let (n,): (i64,) = sqlx::query_as("SELECT count(*) FROM wallets WHERE id=$1 AND organization_id=$2 AND deleted_at IS NULL")
        .bind(id)
        .bind(ctx.org_id)
        .fetch_one(&s.pool)
        .await?;
    if n == 0 {
        return Err(AppError::NotFound("Счёт не найден".into()));
    }
    let mut tx = s.pool.begin().await?;
    add_tx(&mut tx, ctx.org_id, id, dto.amount, "manual", None, dto.comment.as_deref().unwrap_or(""), ctx.user_id).await?;
    tx.commit().await?;
    wallet_one(State(s), ctx, Path(id)).await
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransferDto {
    pub from_wallet_id: Uuid,
    pub to_wallet_id: Uuid,
    pub amount: f64,
    pub comment: Option<String>,
}

async fn wallet_transfer(State(s): State<AppState>, ctx: AuthContext, Json(dto): Json<TransferDto>) -> AppResult<Json<Value>> {
    ctx.require("settings.edit")?;
    if !money(dto.amount) || dto.from_wallet_id == dto.to_wallet_id {
        return Err(AppError::BadRequest("Укажите разные счета и сумму больше нуля".into()));
    }
    let (n,): (i64,) = sqlx::query_as("SELECT count(*) FROM wallets WHERE organization_id=$1 AND deleted_at IS NULL AND id = ANY($2)")
        .bind(ctx.org_id)
        .bind(vec![dto.from_wallet_id, dto.to_wallet_id])
        .fetch_one(&s.pool)
        .await?;
    if n != 2 {
        return Err(AppError::NotFound("Счёт не найден".into()));
    }
    let id = Uuid::new_v4();
    let comment = dto.comment.unwrap_or_default();
    let mut tx = s.pool.begin().await?;
    add_tx(&mut tx, ctx.org_id, dto.from_wallet_id, -dto.amount, "transfer", Some(id), &comment, ctx.user_id).await?;
    add_tx(&mut tx, ctx.org_id, dto.to_wallet_id, dto.amount, "transfer", Some(id), &comment, ctx.user_id).await?;
    tx.commit().await?;
    Ok(Json(json!({ "id": id })))
}

// =========================== расходы ===========================
#[derive(Serialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct SpendingView {
    pub id: Uuid,
    pub store_id: Uuid,
    pub user_id: Uuid,
    pub category_id: Option<Uuid>,
    pub category_name: Option<String>,
    pub details: String,
    pub amount: f64,
    pub wallet_id: Option<Uuid>,
    pub spending_date: NaiveDate,
    pub kind: String,
}

const SELECT_SPENDINGS: &str = "SELECT s.id, s.store_id, s.user_id, s.category_id, c.name AS category_name, s.details, \
     s.amount::float8 AS amount, s.wallet_id, s.spending_date, s.kind \
     FROM spendings s LEFT JOIN list_items c ON c.id = s.category_id \
     WHERE s.organization_id=$1 AND s.deleted_at IS NULL AND ($2::uuid IS NULL OR s.id=$2) AND ($3::uuid IS NULL OR s.store_id=$3) \
       AND ($4::date IS NULL OR s.spending_date >= $4) AND ($5::date IS NULL OR s.spending_date <= $5) \
     ORDER BY s.spending_date DESC, s.created_at DESC LIMIT 500";

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SpendingQuery {
    pub store_id: Option<Uuid>,
    pub from: Option<NaiveDate>,
    pub to: Option<NaiveDate>,
}

async fn fetch_spendings(s: &AppState, org: Uuid, id: Option<Uuid>, q: &SpendingQuery) -> AppResult<Vec<SpendingView>> {
    Ok(sqlx::query_as::<_, SpendingView>(SELECT_SPENDINGS)
        .bind(org)
        .bind(id)
        .bind(q.store_id)
        .bind(q.from)
        .bind(q.to)
        .fetch_all(&s.pool)
        .await?)
}

async fn spendings_list(State(s): State<AppState>, ctx: AuthContext, Query(q): Query<SpendingQuery>) -> AppResult<Json<Vec<SpendingView>>> {
    ctx.require("spending.view")?;
    Ok(Json(fetch_spendings(&s, ctx.org_id, None, &q).await?))
}

async fn spending_one(State(s): State<AppState>, ctx: AuthContext, Path(id): Path<Uuid>) -> AppResult<Json<SpendingView>> {
    ctx.require("spending.view")?;
    fetch_spendings(&s, ctx.org_id, Some(id), &SpendingQuery::default())
        .await?
        .into_iter()
        .next()
        .map(Json)
        .ok_or_else(|| AppError::NotFound("Расход не найден".into()))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpendingDto {
    pub store_id: Uuid,
    pub category_id: Option<Uuid>,
    pub details: Option<String>,
    pub amount: f64,
    pub wallet_id: Option<Uuid>,
    pub spending_date: Option<NaiveDate>,
    /// operating | capital
    pub kind: Option<String>,
}

async fn spending_create(State(s): State<AppState>, ctx: AuthContext, Json(dto): Json<SpendingDto>) -> AppResult<Json<SpendingView>> {
    ctx.require("spending.view")?;
    check_store(&s.pool, &ctx, dto.store_id).await?;
    if !money(dto.amount) {
        return Err(AppError::BadRequest("Сумма должна быть больше нуля".into()));
    }
    let kind = dto.kind.clone().unwrap_or_else(|| "operating".into());
    if !["operating", "capital"].contains(&kind.as_str()) {
        return Err(AppError::BadRequest("Неизвестный тип расхода".into()));
    }
    let category = dto.category_id.ok_or_else(|| AppError::BadRequest("Выберите категорию".into()))?;
    let (n,): (i64,) = sqlx::query_as(
        "SELECT count(*) FROM list_items WHERE id=$1 AND organization_id=$2 AND kind='spending_categories' AND deleted_at IS NULL",
    )
    .bind(category)
    .bind(ctx.org_id)
    .fetch_one(&s.pool)
    .await?;
    if n == 0 {
        return Err(AppError::BadRequest("Категория расходов не найдена".into()));
    }
    if let Some(w) = dto.wallet_id {
        check_wallet(&s.pool, ctx.org_id, w, dto.store_id).await?;
    }
    let id = Uuid::new_v4();
    let mut tx = s.pool.begin().await?;
    sqlx::query(
        "INSERT INTO spendings (id, organization_id, store_id, user_id, category_id, details, amount, wallet_id, spending_date, kind) \
         VALUES ($1,$2,$3,$4,$5,$6,$7::float8,$8,COALESCE($9, current_date),$10)",
    )
    .bind(id)
    .bind(ctx.org_id)
    .bind(dto.store_id)
    .bind(ctx.user_id)
    .bind(category)
    .bind(dto.details.unwrap_or_default())
    .bind(dto.amount)
    .bind(dto.wallet_id)
    .bind(dto.spending_date)
    .bind(&kind)
    .execute(&mut *tx)
    .await?;
    if let Some(w) = dto.wallet_id {
        add_tx(&mut tx, ctx.org_id, w, -dto.amount, "spending", Some(id), "", ctx.user_id).await?;
    }
    tx.commit().await?;
    spending_one(State(s), ctx, Path(id)).await
}

async fn spending_remove(State(s): State<AppState>, ctx: AuthContext, Path(id): Path<Uuid>) -> AppResult<Json<Value>> {
    ctx.require("spending.delete")?;
    let mut tx = s.pool.begin().await?;
    let res = sqlx::query("UPDATE spendings SET deleted_at=now() WHERE id=$1 AND organization_id=$2 AND deleted_at IS NULL")
        .bind(id)
        .bind(ctx.org_id)
        .execute(&mut *tx)
        .await?;
    if res.rows_affected() == 0 {
        return Err(AppError::NotFound("Расход не найден".into()));
    }
    reverse_tx(&mut tx, id, "spending", ctx.user_id).await?;
    tx.commit().await?;
    Ok(ok())
}

// =========================== ежемесячные расходы ===========================
#[derive(Serialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct RecurringView {
    pub id: Uuid,
    pub store_id: Uuid,
    pub name: String,
    pub amount: f64,
    pub start_date: NaiveDate,
    pub end_date: Option<NaiveDate>,
}

const SELECT_RECURRING: &str = "SELECT id, store_id, name, amount::float8 AS amount, start_date, end_date FROM recurring_expenses \
     WHERE organization_id=$1 AND deleted_at IS NULL AND ($2::uuid IS NULL OR id=$2) ORDER BY start_date, created_at";

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecurringDto {
    pub store_id: Uuid,
    pub name: String,
    pub amount: f64,
    pub start_date: NaiveDate,
    pub end_date: Option<NaiveDate>,
}

async fn rec_list(State(s): State<AppState>, ctx: AuthContext) -> AppResult<Json<Vec<RecurringView>>> {
    ctx.require("spending.view")?;
    Ok(Json(sqlx::query_as::<_, RecurringView>(SELECT_RECURRING).bind(ctx.org_id).bind(None::<Uuid>).fetch_all(&s.pool).await?))
}

async fn rec_one(State(s): State<AppState>, ctx: AuthContext, Path(id): Path<Uuid>) -> AppResult<Json<RecurringView>> {
    ctx.require("spending.view")?;
    sqlx::query_as::<_, RecurringView>(SELECT_RECURRING)
        .bind(ctx.org_id)
        .bind(Some(id))
        .fetch_optional(&s.pool)
        .await?
        .map(Json)
        .ok_or_else(|| AppError::NotFound("Расход не найден".into()))
}

async fn check_rec(s: &AppState, ctx: &AuthContext, dto: &RecurringDto) -> AppResult<()> {
    if dto.name.trim().is_empty() {
        return Err(AppError::BadRequest("Введите название".into()));
    }
    if !money(dto.amount) {
        return Err(AppError::BadRequest("Сумма должна быть больше нуля".into()));
    }
    if dto.end_date.is_some_and(|e| e < dto.start_date) {
        return Err(AppError::BadRequest("Дата окончания раньше даты начала".into()));
    }
    check_store(&s.pool, ctx, dto.store_id).await
}

async fn rec_create(State(s): State<AppState>, ctx: AuthContext, Json(dto): Json<RecurringDto>) -> AppResult<Json<RecurringView>> {
    ctx.require("settings.edit")?;
    check_rec(&s, &ctx, &dto).await?;
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO recurring_expenses (id, organization_id, store_id, name, amount, start_date, end_date) VALUES ($1,$2,$3,$4,$5::float8,$6,$7)",
    )
    .bind(id)
    .bind(ctx.org_id)
    .bind(dto.store_id)
    .bind(dto.name.trim())
    .bind(dto.amount)
    .bind(dto.start_date)
    .bind(dto.end_date)
    .execute(&s.pool)
    .await?;
    rec_one(State(s), ctx, Path(id)).await
}

async fn rec_update(
    State(s): State<AppState>,
    ctx: AuthContext,
    Path(id): Path<Uuid>,
    Json(dto): Json<RecurringDto>,
) -> AppResult<Json<RecurringView>> {
    ctx.require("settings.edit")?;
    check_rec(&s, &ctx, &dto).await?;
    let res = sqlx::query(
        "UPDATE recurring_expenses SET store_id=$3, name=$4, amount=$5::float8, start_date=$6, end_date=$7 \
         WHERE id=$1 AND organization_id=$2 AND deleted_at IS NULL",
    )
    .bind(id)
    .bind(ctx.org_id)
    .bind(dto.store_id)
    .bind(dto.name.trim())
    .bind(dto.amount)
    .bind(dto.start_date)
    .bind(dto.end_date)
    .execute(&s.pool)
    .await?;
    if res.rows_affected() == 0 {
        return Err(AppError::NotFound("Расход не найден".into()));
    }
    rec_one(State(s), ctx, Path(id)).await
}

async fn rec_remove(State(s): State<AppState>, ctx: AuthContext, Path(id): Path<Uuid>) -> AppResult<Json<Value>> {
    ctx.require("settings.edit")?;
    let res = sqlx::query("UPDATE recurring_expenses SET deleted_at=now() WHERE id=$1 AND organization_id=$2 AND deleted_at IS NULL")
        .bind(id)
        .bind(ctx.org_id)
        .execute(&s.pool)
        .await?;
    if res.rows_affected() == 0 {
        return Err(AppError::NotFound("Расход не найден".into()));
    }
    Ok(ok())
}
