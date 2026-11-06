//! Зарплата: /api/salary, /api/salary-payments, /api/working-hours
use crate::{
    auth::AuthContext,
    error::{AppError, AppResult},
    routes::finance::{add_tx, reverse_tx},
    AppState,
};
use atlas_domain::{
    finance::monthly_share,
    sale::round2,
    salary::{base_pay, bonus_pay},
};
use axum::{
    extract::{Path, Query, State},
    routing::{delete, get, post},
    Json, Router,
};
use chrono::{DateTime, Datelike, Duration, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

pub fn router() -> Router<AppState> {
    Router::new().route("/", get(list)).route("/{user_id}", get(detail)).route("/{user_id}/payments", post(pay))
}

pub fn payments_router() -> Router<AppState> {
    Router::new().route("/{id}", delete(pay_remove))
}

pub fn hours_router() -> Router<AppState> {
    Router::new().route("/", get(hours_list).post(hours_create)).route("/{id}", axum::routing::put(hours_update).delete(hours_remove))
}

#[derive(FromRow, Clone)]
pub struct UserPay {
    pub id: Uuid,
    pub name: String,
    pub role_name: String,
    pub salary_type: String,
    pub salary_rate: f64,
    pub bonus_type: String,
    pub bonus_rate: f64,
    pub store_ids: Vec<Uuid>,
    pub created_at: DateTime<Utc>,
}

const SELECT_USERS: &str = "SELECT u.id, u.name, r.name AS role_name, r.salary_type, r.salary_rate::float8 AS salary_rate, r.bonus_type, \
     r.bonus_rate::float8 AS bonus_rate, u.store_ids, u.created_at FROM users u JOIN roles r ON r.id = u.role_id \
     WHERE u.organization_id=$1 AND ($2::uuid IS NULL OR u.id=$2) ORDER BY u.name";

pub struct Accrual {
    pub hours: f64,
    pub shifts: i64,
    pub base: f64,
    pub bonus: f64,
}

impl Accrual {
    pub fn total(&self) -> f64 {
        round2(self.base + self.bonus)
    }
}

fn last_day(to: DateTime<Utc>) -> NaiveDate {
    (to - Duration::seconds(1)).date_naive()
}

/// Начислено сотруднику за период `[from, to)` по выбранным филиалам.
pub async fn accrual(pool: &PgPool, org: Uuid, u: &UserPay, from: DateTime<Utc>, to: DateTime<Utc>, stores: &[Uuid]) -> AppResult<Accrual> {
    let (hours, shifts): (f64, i64) = sqlx::query_as(
        "SELECT COALESCE(SUM(EXTRACT(EPOCH FROM (COALESCE(ended_at, now()) - started_at))) / 3600, 0)::float8, COUNT(*) \
         FROM working_hours WHERE organization_id=$1 AND user_id=$2 AND started_at >= $3 AND started_at < $4 AND store_id = ANY($5)",
    )
    .bind(org)
    .bind(u.id)
    .bind(from)
    .bind(to)
    .bind(stores)
    .fetch_one(pool)
    .await?;
    let month = if u.salary_type == "month" && u.store_ids.iter().any(|s| stores.contains(s)) {
        monthly_share(u.salary_rate, u.created_at.date_naive(), None, from.date_naive(), last_day(to))
    } else {
        0.0
    };
    let (own_sales,): (f64,) = sqlx::query_as(
        "SELECT COALESCE(SUM(total - returned_amount),0)::float8 FROM sales WHERE organization_id=$1 AND user_id=$2 AND deleted_at IS NULL \
         AND sale_date >= $3 AND sale_date < $4 AND store_id = ANY($5)",
    )
    .bind(org)
    .bind(u.id)
    .bind(from)
    .bind(to)
    .bind(stores)
    .fetch_one(pool)
    .await?;
    Ok(Accrual {
        hours: round2(hours),
        shifts,
        base: base_pay(&u.salary_type, u.salary_rate, hours, shifts, month),
        bonus: bonus_pay(&u.bonus_type, u.bonus_rate, own_sales),
    })
}

/// «Зарплаты» для P&L: начислено всем сотрудникам за период по филиалам.
pub async fn total_accrued(pool: &PgPool, org: Uuid, from: DateTime<Utc>, to: DateTime<Utc>, stores: &[Uuid]) -> AppResult<f64> {
    let users = sqlx::query_as::<_, UserPay>(SELECT_USERS).bind(org).bind(None::<Uuid>).fetch_all(pool).await?;
    let mut sum = 0.0;
    for u in &users {
        sum += accrual(pool, org, u, from, to, stores).await?.total();
    }
    Ok(round2(sum))
}

async fn paid_in(pool: &PgPool, org: Uuid, user: Uuid, from: NaiveDate, to: NaiveDate) -> AppResult<f64> {
    let (v,): (f64,) = sqlx::query_as(
        "SELECT COALESCE(SUM(amount),0)::float8 FROM salary_payments WHERE organization_id=$1 AND user_id=$2 AND deleted_at IS NULL \
         AND paid_on >= $3 AND paid_on <= $4",
    )
    .bind(org)
    .bind(user)
    .bind(from)
    .bind(to)
    .fetch_one(pool)
    .await?;
    Ok(v)
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PeriodQuery {
    pub from: Option<DateTime<Utc>>,
    pub to: Option<DateTime<Utc>>,
    pub store_id: Option<Uuid>,
}

/// Период по умолчанию — текущий месяц (UTC).
fn period(q: &PeriodQuery) -> (DateTime<Utc>, DateTime<Utc>) {
    let today = Utc::now().date_naive();
    let first = NaiveDate::from_ymd_opt(today.year(), today.month(), 1).expect("first day");
    let next = if today.month() == 12 { NaiveDate::from_ymd_opt(today.year() + 1, 1, 1) } else { NaiveDate::from_ymd_opt(today.year(), today.month() + 1, 1) }
        .expect("next month");
    let at = |d: NaiveDate| d.and_hms_opt(0, 0, 0).expect("midnight").and_utc();
    (q.from.unwrap_or_else(|| at(first)), q.to.unwrap_or_else(|| at(next)))
}

async fn scope_stores(pool: &PgPool, org: Uuid, store: Option<Uuid>) -> AppResult<Vec<Uuid>> {
    match store {
        Some(s) => Ok(vec![s]),
        None => {
            let rows: Vec<(Uuid,)> = sqlx::query_as("SELECT id FROM stores WHERE organization_id=$1 AND deleted_at IS NULL").bind(org).fetch_all(pool).await?;
            Ok(rows.into_iter().map(|r| r.0).collect())
        }
    }
}

async fn summary(pool: &PgPool, org: Uuid, u: &UserPay, from: DateTime<Utc>, to: DateTime<Utc>, stores: &[Uuid]) -> AppResult<Value> {
    let a = accrual(pool, org, u, from, to, stores).await?;
    let paid = paid_in(pool, org, u.id, from.date_naive(), last_day(to)).await?;
    Ok(json!({
        "userId": u.id, "name": u.name, "roleName": u.role_name,
        "salaryType": u.salary_type, "rate": u.salary_rate, "bonusType": u.bonus_type, "bonusRate": u.bonus_rate,
        "hours": a.hours, "shifts": a.shifts, "base": a.base, "bonus": a.bonus,
        "accrued": a.total(), "paid": round2(paid), "toPay": round2(a.total() - paid),
    }))
}

async fn list(State(s): State<AppState>, ctx: AuthContext, Query(q): Query<PeriodQuery>) -> AppResult<Json<Vec<Value>>> {
    ctx.require("salary.view")?;
    let (from, to) = period(&q);
    let stores = scope_stores(&s.pool, ctx.org_id, q.store_id).await?;
    let only = if ctx.can("salary.view_others") { None } else { Some(ctx.user_id) };
    let users = sqlx::query_as::<_, UserPay>(SELECT_USERS).bind(ctx.org_id).bind(only).fetch_all(&s.pool).await?;
    let mut out = Vec::new();
    for u in &users {
        out.push(summary(&s.pool, ctx.org_id, u, from, to, &stores).await?);
    }
    Ok(Json(out))
}

async fn detail(State(s): State<AppState>, ctx: AuthContext, Path(user_id): Path<Uuid>, Query(q): Query<PeriodQuery>) -> AppResult<Json<Value>> {
    ctx.require("salary.view")?;
    if user_id != ctx.user_id {
        ctx.require("salary.view_others")?;
    }
    let (from, to) = period(&q);
    let stores = scope_stores(&s.pool, ctx.org_id, q.store_id).await?;
    let u = sqlx::query_as::<_, UserPay>(SELECT_USERS)
        .bind(ctx.org_id)
        .bind(Some(user_id))
        .fetch_optional(&s.pool)
        .await?
        .ok_or_else(|| AppError::NotFound("Сотрудник не найден".into()))?;
    let mut v = summary(&s.pool, ctx.org_id, &u, from, to, &stores).await?;
    // баланс за всё время: начислено с регистрации сотрудника минус все выплаты
    let all_from = u.created_at;
    let all_to = Utc::now() + Duration::days(1);
    let all = accrual(&s.pool, ctx.org_id, &u, all_from, all_to, &scope_stores(&s.pool, ctx.org_id, None).await?).await?;
    let all_paid = paid_in(&s.pool, ctx.org_id, user_id, all_from.date_naive(), all_to.date_naive()).await?;
    let hours = sqlx::query_as::<_, HoursRow>(
        "SELECT id, store_id, user_id, shift_id, started_at, ended_at FROM working_hours \
         WHERE organization_id=$1 AND user_id=$2 AND started_at >= $3 AND started_at < $4 ORDER BY started_at",
    )
    .bind(ctx.org_id)
    .bind(user_id)
    .bind(from)
    .bind(to)
    .fetch_all(&s.pool)
    .await?;
    let payments = sqlx::query_as::<_, PaymentRow>(
        "SELECT id, amount::float8 AS amount, wallet_id, comment, paid_on FROM salary_payments \
         WHERE organization_id=$1 AND user_id=$2 AND deleted_at IS NULL AND paid_on >= $3 AND paid_on <= $4 ORDER BY paid_on, created_at",
    )
    .bind(ctx.org_id)
    .bind(user_id)
    .bind(from.date_naive())
    .bind(last_day(to))
    .fetch_all(&s.pool)
    .await?;
    v["balancePeriod"] = json!(v["toPay"].clone());
    v["balanceAll"] = json!(round2(all.total() - all_paid));
    v["workingHours"] = serde_json::to_value(hours).map_err(|e| AppError::Internal(e.to_string()))?;
    v["payments"] = serde_json::to_value(payments).map_err(|e| AppError::Internal(e.to_string()))?;
    Ok(Json(v))
}

#[derive(Serialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct PaymentRow {
    pub id: Uuid,
    pub amount: f64,
    pub wallet_id: Option<Uuid>,
    pub comment: String,
    pub paid_on: NaiveDate,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PayDto {
    pub amount: f64,
    pub wallet_id: Option<Uuid>,
    pub comment: Option<String>,
    pub paid_on: Option<NaiveDate>,
}

/// «Выплатить»: создаёт выплату и списывает деньги с выбранного денежного счёта.
async fn pay(State(s): State<AppState>, ctx: AuthContext, Path(user_id): Path<Uuid>, Json(dto): Json<PayDto>) -> AppResult<Json<PaymentRow>> {
    ctx.require("salary.view_others")?;
    if !(dto.amount.is_finite() && dto.amount > 0.0) {
        return Err(AppError::BadRequest("Сумма должна быть больше нуля".into()));
    }
    let (n,): (i64,) = sqlx::query_as("SELECT count(*) FROM users WHERE id=$1 AND organization_id=$2").bind(user_id).bind(ctx.org_id).fetch_one(&s.pool).await?;
    if n == 0 {
        return Err(AppError::NotFound("Сотрудник не найден".into()));
    }
    if let Some(w) = dto.wallet_id {
        let (n,): (i64,) = sqlx::query_as("SELECT count(*) FROM wallets WHERE id=$1 AND organization_id=$2 AND deleted_at IS NULL")
            .bind(w)
            .bind(ctx.org_id)
            .fetch_one(&s.pool)
            .await?;
        if n == 0 {
            return Err(AppError::BadRequest("Денежный счёт не найден".into()));
        }
    }
    let id = Uuid::new_v4();
    let mut tx = s.pool.begin().await?;
    sqlx::query(
        "INSERT INTO salary_payments (id, organization_id, user_id, amount, wallet_id, comment, paid_on, created_by) \
         VALUES ($1,$2,$3,$4::float8,$5,$6,COALESCE($7, current_date),$8)",
    )
    .bind(id)
    .bind(ctx.org_id)
    .bind(user_id)
    .bind(dto.amount)
    .bind(dto.wallet_id)
    .bind(dto.comment.clone().unwrap_or_default())
    .bind(dto.paid_on)
    .bind(ctx.user_id)
    .execute(&mut *tx)
    .await?;
    if let Some(w) = dto.wallet_id {
        add_tx(&mut tx, ctx.org_id, w, -dto.amount, "salary", Some(id), "Зарплата", ctx.user_id).await?;
    }
    tx.commit().await?;
    let row = sqlx::query_as::<_, PaymentRow>("SELECT id, amount::float8 AS amount, wallet_id, comment, paid_on FROM salary_payments WHERE id=$1")
        .bind(id)
        .fetch_one(&s.pool)
        .await?;
    Ok(Json(row))
}

async fn pay_remove(State(s): State<AppState>, ctx: AuthContext, Path(id): Path<Uuid>) -> AppResult<Json<Value>> {
    ctx.require("salary.view_others")?;
    let mut tx = s.pool.begin().await?;
    let res = sqlx::query("UPDATE salary_payments SET deleted_at=now() WHERE id=$1 AND organization_id=$2 AND deleted_at IS NULL")
        .bind(id)
        .bind(ctx.org_id)
        .execute(&mut *tx)
        .await?;
    if res.rows_affected() == 0 {
        return Err(AppError::NotFound("Выплата не найдена".into()));
    }
    reverse_tx(&mut tx, id, "salary", ctx.user_id).await?;
    tx.commit().await?;
    Ok(Json(json!({ "success": true })))
}

// =========================== рабочие часы ===========================
#[derive(Serialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct HoursRow {
    pub id: Uuid,
    pub store_id: Uuid,
    pub user_id: Uuid,
    pub shift_id: Option<Uuid>,
    pub started_at: DateTime<Utc>,
    pub ended_at: Option<DateTime<Utc>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HoursQuery {
    pub user_id: Option<Uuid>,
    pub from: Option<DateTime<Utc>>,
    pub to: Option<DateTime<Utc>>,
}

async fn hours_list(State(s): State<AppState>, ctx: AuthContext, Query(q): Query<HoursQuery>) -> AppResult<Json<Vec<HoursRow>>> {
    ctx.require("salary.view")?;
    let user = q.user_id.unwrap_or(ctx.user_id);
    if user != ctx.user_id {
        ctx.require("salary.view_others")?;
    }
    Ok(Json(
        sqlx::query_as::<_, HoursRow>(
            "SELECT id, store_id, user_id, shift_id, started_at, ended_at FROM working_hours \
             WHERE organization_id=$1 AND user_id=$2 AND ($3::timestamptz IS NULL OR started_at >= $3) AND ($4::timestamptz IS NULL OR started_at < $4) \
             ORDER BY started_at DESC LIMIT 500",
        )
        .bind(ctx.org_id)
        .bind(user)
        .bind(q.from)
        .bind(q.to)
        .fetch_all(&s.pool)
        .await?,
    ))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HoursDto {
    pub user_id: Option<Uuid>,
    pub store_id: Uuid,
    pub started_at: DateTime<Utc>,
    pub ended_at: Option<DateTime<Utc>>,
}

async fn check_hours(s: &AppState, ctx: &AuthContext, user: Uuid, dto: &HoursDto) -> AppResult<()> {
    ctx.require("salary.view")?;
    if user != ctx.user_id {
        ctx.require("shift.edit_staff")?;
    }
    if dto.ended_at.is_some_and(|e| e < dto.started_at) {
        return Err(AppError::BadRequest("Окончание раньше начала".into()));
    }
    let (n,): (i64,) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM users WHERE id=$1 AND organization_id=$2) + (SELECT count(*) FROM stores WHERE id=$3 AND organization_id=$2 AND deleted_at IS NULL)",
    )
    .bind(user)
    .bind(ctx.org_id)
    .bind(dto.store_id)
    .fetch_one(&s.pool)
    .await?;
    if n != 2 {
        return Err(AppError::BadRequest("Сотрудник или филиал не найден".into()));
    }
    Ok(())
}

async fn hours_create(State(s): State<AppState>, ctx: AuthContext, Json(dto): Json<HoursDto>) -> AppResult<Json<HoursRow>> {
    let user = dto.user_id.unwrap_or(ctx.user_id);
    check_hours(&s, &ctx, user, &dto).await?;
    let id = Uuid::new_v4();
    sqlx::query("INSERT INTO working_hours (id, organization_id, store_id, user_id, started_at, ended_at, created_by) VALUES ($1,$2,$3,$4,$5,$6,$7)")
        .bind(id)
        .bind(ctx.org_id)
        .bind(dto.store_id)
        .bind(user)
        .bind(dto.started_at)
        .bind(dto.ended_at)
        .bind(ctx.user_id)
        .execute(&s.pool)
        .await?;
    hours_one(&s.pool, ctx.org_id, id).await
}

async fn hours_one(pool: &PgPool, org: Uuid, id: Uuid) -> AppResult<Json<HoursRow>> {
    sqlx::query_as::<_, HoursRow>("SELECT id, store_id, user_id, shift_id, started_at, ended_at FROM working_hours WHERE id=$1 AND organization_id=$2")
        .bind(id)
        .bind(org)
        .fetch_optional(pool)
        .await?
        .map(Json)
        .ok_or_else(|| AppError::NotFound("Запись не найдена".into()))
}

async fn hours_owner(pool: &PgPool, org: Uuid, id: Uuid) -> AppResult<Uuid> {
    let r: Option<(Uuid,)> = sqlx::query_as("SELECT user_id FROM working_hours WHERE id=$1 AND organization_id=$2").bind(id).bind(org).fetch_optional(pool).await?;
    r.map(|x| x.0).ok_or_else(|| AppError::NotFound("Запись не найдена".into()))
}

async fn hours_update(State(s): State<AppState>, ctx: AuthContext, Path(id): Path<Uuid>, Json(dto): Json<HoursDto>) -> AppResult<Json<HoursRow>> {
    let owner = hours_owner(&s.pool, ctx.org_id, id).await?;
    check_hours(&s, &ctx, owner, &dto).await?;
    sqlx::query("UPDATE working_hours SET store_id=$2, started_at=$3, ended_at=$4 WHERE id=$1")
        .bind(id)
        .bind(dto.store_id)
        .bind(dto.started_at)
        .bind(dto.ended_at)
        .execute(&s.pool)
        .await?;
    hours_one(&s.pool, ctx.org_id, id).await
}

async fn hours_remove(State(s): State<AppState>, ctx: AuthContext, Path(id): Path<Uuid>) -> AppResult<Json<Value>> {
    ctx.require("salary.view")?;
    let owner = hours_owner(&s.pool, ctx.org_id, id).await?;
    if owner != ctx.user_id {
        ctx.require("shift.edit_staff")?;
    }
    sqlx::query("DELETE FROM working_hours WHERE id=$1").bind(id).execute(&s.pool).await?;
    Ok(Json(json!({ "success": true })))
}
