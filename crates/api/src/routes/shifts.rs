//! /api/shifts — смены: открытие/закрытие, касса (внесение/изъятие), сотрудники в смене, автозакрытие
use crate::{
    auth::AuthContext,
    error::{AppError, AppResult},
    routes::sales::check_store,
    AppState,
};
use atlas_domain::{
    sale::round2,
    salary::{cash_difference, expected_cash, CashInput},
};
use axum::{
    extract::{Path, Query, State},
    routing::{delete, get, post},
    Json, Router,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list))
        .route("/current", get(current))
        .route("/open", post(open))
        .route("/{id}", get(one))
        .route("/{id}/close", post(close))
        .route("/{id}/reopen", post(reopen))
        .route("/{id}/staff", post(staff_add))
        .route("/{id}/staff/{user_id}", delete(staff_remove))
        .route("/{id}/cashbox", post(cashbox))
}

#[derive(Serialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct ShiftRow {
    pub id: Uuid,
    pub store_id: Uuid,
    pub opened_by: Uuid,
    pub opened_at: DateTime<Utc>,
    pub closed_at: Option<DateTime<Utc>>,
    pub closed_by: Option<Uuid>,
    pub opening_cash: f64,
    pub expected_cash: Option<f64>,
    pub actual_cash: Option<f64>,
    pub diff: Option<f64>,
    pub comment: String,
    pub auto_closed: bool,
}

#[derive(Serialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct StaffView {
    #[serde(skip)]
    pub shift_id: Uuid,
    pub user_id: Uuid,
    pub name: String,
    pub added_at: DateTime<Utc>,
}

#[derive(Serialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct CashboxView {
    #[serde(skip)]
    pub shift_id: Uuid,
    pub id: Uuid,
    pub amount: f64,
    pub comment: String,
    pub user_id: Uuid,
    pub created_at: DateTime<Utc>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShiftTotals {
    pub sales: f64,
    pub checks: i64,
    pub cash_sales: f64,
    pub non_cash_sales: f64,
    pub cash_returns: f64,
    pub cash_spendings: f64,
    pub paid_supplies: f64,
    pub deposits: f64,
    pub withdrawals: f64,
    pub expected_cash: f64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShiftView {
    #[serde(flatten)]
    pub shift: ShiftRow,
    pub staff: Vec<StaffView>,
    pub cashbox: Vec<CashboxView>,
    pub totals: ShiftTotals,
}

const SELECT_SHIFTS: &str = "SELECT id, store_id, opened_by, opened_at, closed_at, closed_by, opening_cash::float8 AS opening_cash, \
     expected_cash::float8 AS expected_cash, actual_cash::float8 AS actual_cash, diff::float8 AS diff, comment, auto_closed \
     FROM shifts WHERE organization_id=$1 AND ($2::uuid IS NULL OR id=$2) AND ($3::uuid IS NULL OR store_id=$3) \
       AND ($4::timestamptz IS NULL OR opened_at >= $4) AND ($5::timestamptz IS NULL OR opened_at < $5) \
       AND (NOT $6 OR closed_at IS NULL) ORDER BY opened_at DESC LIMIT 500";

/// Итоги смены за окно `[opened_at, end)`. Для открытой смены `end` — текущий момент.
pub async fn totals(pool: &PgPool, shift: &ShiftRow, end: DateTime<Utc>) -> AppResult<ShiftTotals> {
    let (sales, checks, cash_sales): (f64, i64, f64) = sqlx::query_as(
        "SELECT COALESCE(SUM(s.total),0)::float8, COUNT(*), \
                COALESCE(SUM(CASE WHEN s.second_payment_method_id IS NULL \
                    THEN (CASE WHEN p1.params->>'kind' = 'cash' THEN s.total ELSE 0 END) \
                    ELSE (CASE WHEN p1.params->>'kind' = 'cash' THEN s.first_payment_value ELSE 0 END) \
                       + (CASE WHEN p2.params->>'kind' = 'cash' THEN s.second_payment_value ELSE 0 END) END),0)::float8 \
         FROM sales s LEFT JOIN list_items p1 ON p1.id = s.payment_method_id LEFT JOIN list_items p2 ON p2.id = s.second_payment_method_id \
         WHERE s.shift_id = $1 AND s.deleted_at IS NULL",
    )
    .bind(shift.id)
    .fetch_one(pool)
    .await?;
    // возвраты денег по продажам, оплаченным наличными одним методом, сделанные в окне смены
    let (cash_returns,): (f64,) = sqlx::query_as(
        "SELECT COALESCE(SUM(r.amount),0)::float8 FROM sale_returns r JOIN sales s ON s.id = r.sale_id \
         LEFT JOIN list_items p1 ON p1.id = s.payment_method_id \
         WHERE s.store_id=$1 AND r.return_type='money' AND s.second_payment_method_id IS NULL AND p1.params->>'kind' = 'cash' \
           AND r.created_at >= $2 AND r.created_at < $3",
    )
    .bind(shift.store_id)
    .bind(shift.opened_at)
    .bind(end)
    .fetch_one(pool)
    .await?;
    let (cash_spendings,): (f64,) = sqlx::query_as(
        "SELECT COALESCE(SUM(sp.amount),0)::float8 FROM spendings sp JOIN wallets w ON w.id = sp.wallet_id \
         WHERE sp.store_id=$1 AND sp.deleted_at IS NULL AND w.is_cash_register AND sp.created_at >= $2 AND sp.created_at < $3",
    )
    .bind(shift.store_id)
    .bind(shift.opened_at)
    .bind(end)
    .fetch_one(pool)
    .await?;
    let (paid_supplies,): (f64,) = sqlx::query_as(
        "SELECT COALESCE(SUM(-t.delta),0)::float8 FROM wallet_transactions t JOIN wallets w ON w.id = t.wallet_id \
         WHERE t.kind='supply_payment' AND w.is_cash_register AND w.store_id=$1 AND t.created_at >= $2 AND t.created_at < $3",
    )
    .bind(shift.store_id)
    .bind(shift.opened_at)
    .bind(end)
    .fetch_one(pool)
    .await?;
    let (deposits, withdrawals): (f64, f64) = sqlx::query_as(
        "SELECT COALESCE(SUM(amount) FILTER (WHERE amount > 0),0)::float8, COALESCE(-SUM(amount) FILTER (WHERE amount < 0),0)::float8 \
         FROM cashbox_transactions WHERE shift_id=$1",
    )
    .bind(shift.id)
    .fetch_one(pool)
    .await?;
    let expected = expected_cash(&CashInput {
        opening: shift.opening_cash,
        cash_sales,
        cash_returns,
        withdrawals,
        cash_spendings,
        paid_supplies,
        deposits,
    });
    Ok(ShiftTotals {
        sales: round2(sales),
        checks,
        cash_sales: round2(cash_sales),
        non_cash_sales: round2(sales - cash_sales),
        cash_returns: round2(cash_returns),
        cash_spendings: round2(cash_spendings),
        paid_supplies: round2(paid_supplies),
        deposits: round2(deposits),
        withdrawals: round2(withdrawals),
        expected_cash: expected,
    })
}

async fn view(pool: &PgPool, row: ShiftRow) -> AppResult<ShiftView> {
    let end = row.closed_at.unwrap_or_else(Utc::now);
    let t = totals(pool, &row, end).await?;
    let staff = sqlx::query_as::<_, StaffView>(
        "SELECT ss.shift_id, ss.user_id, u.name, ss.added_at FROM shift_staff ss JOIN users u ON u.id = ss.user_id \
         WHERE ss.shift_id=$1 ORDER BY ss.added_at",
    )
    .bind(row.id)
    .fetch_all(pool)
    .await?;
    let cashbox = sqlx::query_as::<_, CashboxView>(
        "SELECT shift_id, id, amount::float8 AS amount, comment, user_id, created_at FROM cashbox_transactions \
         WHERE shift_id=$1 ORDER BY created_at",
    )
    .bind(row.id)
    .fetch_all(pool)
    .await?;
    Ok(ShiftView { shift: row, staff, cashbox, totals: t })
}

async fn fetch_rows(pool: &PgPool, org: Uuid, id: Option<Uuid>, q: &ListQuery) -> AppResult<Vec<ShiftRow>> {
    Ok(sqlx::query_as::<_, ShiftRow>(SELECT_SHIFTS)
        .bind(org)
        .bind(id)
        .bind(q.store_id)
        .bind(q.from)
        .bind(q.to)
        .bind(q.open_only)
        .fetch_all(pool)
        .await?)
}

async fn view_one(pool: &PgPool, org: Uuid, id: Uuid) -> AppResult<ShiftView> {
    let row = fetch_rows(pool, org, Some(id), &ListQuery::default())
        .await?
        .into_iter()
        .next()
        .ok_or_else(|| AppError::NotFound("Смена не найдена".into()))?;
    view(pool, row).await
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ListQuery {
    pub store_id: Option<Uuid>,
    pub from: Option<DateTime<Utc>>,
    pub to: Option<DateTime<Utc>>,
    #[serde(default)]
    pub open_only: bool,
}

async fn list(State(s): State<AppState>, ctx: AuthContext, Query(q): Query<ListQuery>) -> AppResult<Json<Vec<ShiftView>>> {
    ctx.require("shift.view")?;
    let rows = fetch_rows(&s.pool, ctx.org_id, None, &q).await?;
    let mut out = Vec::new();
    for r in rows {
        out.push(view(&s.pool, r).await?);
    }
    Ok(Json(out))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CurrentQuery {
    pub store_id: Uuid,
}

/// Открытая смена филиала или `null`.
async fn current(State(s): State<AppState>, ctx: AuthContext, Query(q): Query<CurrentQuery>) -> AppResult<Json<Value>> {
    ctx.require("shift.view")?;
    check_store(&s.pool, &ctx, q.store_id).await?;
    let rows = fetch_rows(&s.pool, ctx.org_id, None, &ListQuery { store_id: Some(q.store_id), open_only: true, ..Default::default() }).await?;
    match rows.into_iter().next() {
        Some(r) => Ok(Json(serde_json::to_value(view(&s.pool, r).await?).map_err(|e| AppError::Internal(e.to_string()))?)),
        None => Ok(Json(Value::Null)),
    }
}

async fn one(State(s): State<AppState>, ctx: AuthContext, Path(id): Path<Uuid>) -> AppResult<Json<ShiftView>> {
    ctx.require("shift.view")?;
    Ok(Json(view_one(&s.pool, ctx.org_id, id).await?))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenDto {
    pub store_id: Uuid,
    /// «Наличных в кассе на начало»; по умолчанию — остаток счёта «Наличные с кассы» филиала
    pub opening_cash: Option<f64>,
    #[serde(default)]
    pub staff_ids: Vec<Uuid>,
}

/// Запись рабочих часов для ролей с почасовой и сменной оплатой.
async fn start_hours(tx: &mut sqlx::Transaction<'_, sqlx::Postgres>, org: Uuid, store: Uuid, shift: Uuid, user: Uuid, by: Uuid) -> AppResult<()> {
    sqlx::query(
        "INSERT INTO working_hours (id, organization_id, store_id, user_id, shift_id, started_at, created_by) \
         SELECT gen_random_uuid(), $1, $2, u.id, $3, now(), $5 FROM users u JOIN roles r ON r.id = u.role_id \
         WHERE u.id=$4 AND u.organization_id=$1 AND r.salary_type IN ('hour','day')",
    )
    .bind(org)
    .bind(store)
    .bind(shift)
    .bind(user)
    .bind(by)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

async fn open(State(s): State<AppState>, ctx: AuthContext, Json(dto): Json<OpenDto>) -> AppResult<Json<ShiftView>> {
    ctx.require("shift.view")?;
    check_store(&s.pool, &ctx, dto.store_id).await?;
    let (open,): (i64,) = sqlx::query_as("SELECT count(*) FROM shifts WHERE store_id=$1 AND closed_at IS NULL")
        .bind(dto.store_id)
        .fetch_one(&s.pool)
        .await?;
    if open > 0 {
        return Err(AppError::Conflict("Смена уже открыта".into()));
    }
    let opening = match dto.opening_cash {
        Some(v) if v.is_finite() && v >= 0.0 => v,
        Some(_) => return Err(AppError::BadRequest("Наличных в кассе не может быть меньше нуля".into())),
        None => {
            let (v,): (f64,) = sqlx::query_as(
                "SELECT COALESCE(SUM(t.delta),0)::float8 FROM wallet_transactions t JOIN wallets w ON w.id = t.wallet_id \
                 WHERE w.is_cash_register AND w.store_id=$1 AND w.deleted_at IS NULL",
            )
            .bind(dto.store_id)
            .fetch_one(&s.pool)
            .await?;
            v.max(0.0)
        }
    };
    let mut staff = dto.staff_ids.clone();
    if !staff.contains(&ctx.user_id) {
        staff.push(ctx.user_id);
    }
    let (found,): (i64,) = sqlx::query_as("SELECT count(*) FROM users WHERE organization_id=$1 AND active AND id = ANY($2)")
        .bind(ctx.org_id)
        .bind(&staff)
        .fetch_one(&s.pool)
        .await?;
    if found as usize != staff.len() {
        return Err(AppError::BadRequest("Сотрудник не найден".into()));
    }
    let id = Uuid::new_v4();
    let mut tx = s.pool.begin().await?;
    sqlx::query("INSERT INTO shifts (id, organization_id, store_id, opened_by, opening_cash) VALUES ($1,$2,$3,$4,$5::float8)")
        .bind(id)
        .bind(ctx.org_id)
        .bind(dto.store_id)
        .bind(ctx.user_id)
        .bind(opening)
        .execute(&mut *tx)
        .await?;
    for u in &staff {
        sqlx::query("INSERT INTO shift_staff (shift_id, user_id) VALUES ($1,$2)").bind(id).bind(u).execute(&mut *tx).await?;
        start_hours(&mut tx, ctx.org_id, dto.store_id, id, *u, ctx.user_id).await?;
    }
    tx.commit().await?;
    Ok(Json(view_one(&s.pool, ctx.org_id, id).await?))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloseDto {
    /// «Фактически в кассе»
    pub actual_cash: f64,
    pub comment: Option<String>,
}

/// Закрытие смены: расчёт наличных, разница «факт − расчёт», завершение рабочих часов.
async fn close_shift(pool: &PgPool, org: Uuid, id: Uuid, actual: Option<f64>, comment: &str, user: Option<Uuid>, auto: bool, now: DateTime<Utc>) -> AppResult<()> {
    let row = fetch_rows(pool, org, Some(id), &ListQuery::default())
        .await?
        .into_iter()
        .next()
        .ok_or_else(|| AppError::NotFound("Смена не найдена".into()))?;
    if row.closed_at.is_some() {
        return Err(AppError::BadRequest("Смена уже закрыта".into()));
    }
    let t = totals(pool, &row, now).await?;
    let actual = actual.unwrap_or(t.expected_cash);
    let diff = cash_difference(actual, t.expected_cash);
    let mut tx = pool.begin().await?;
    let res = sqlx::query(
        "UPDATE shifts SET closed_at=$2, closed_by=$3, expected_cash=$4::float8, actual_cash=$5::float8, diff=$6::float8, comment=$7, auto_closed=$8 \
         WHERE id=$1 AND closed_at IS NULL",
    )
    .bind(id)
    .bind(now)
    .bind(user)
    .bind(t.expected_cash)
    .bind(actual)
    .bind(diff)
    .bind(comment)
    .bind(auto)
    .execute(&mut *tx)
    .await?;
    if res.rows_affected() == 0 {
        return Err(AppError::BadRequest("Смена уже закрыта".into()));
    }
    sqlx::query("UPDATE working_hours SET ended_at=GREATEST($2, started_at) WHERE shift_id=$1 AND ended_at IS NULL")
        .bind(id)
        .bind(now)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(())
}

async fn close(
    State(s): State<AppState>,
    ctx: AuthContext,
    Path(id): Path<Uuid>,
    Json(dto): Json<CloseDto>,
) -> AppResult<Json<ShiftView>> {
    ctx.require("shift.view")?;
    if !(dto.actual_cash.is_finite() && dto.actual_cash >= 0.0) {
        return Err(AppError::BadRequest("Фактически в кассе: введите сумму".into()));
    }
    close_shift(&s.pool, ctx.org_id, id, Some(dto.actual_cash), dto.comment.as_deref().unwrap_or(""), Some(ctx.user_id), false, Utc::now()).await?;
    Ok(Json(view_one(&s.pool, ctx.org_id, id).await?))
}

/// Автозакрытие: смены, у которых наступило время автозакрытия филиала (в его часовом поясе). Возвращает число закрытых.
pub async fn auto_close_due(pool: &PgPool, now: DateTime<Utc>) -> AppResult<usize> {
    let due: Vec<(Uuid, Uuid)> = sqlx::query_as(
        "SELECT s.id, s.organization_id FROM shifts s JOIN stores st ON st.id = s.store_id \
         WHERE s.closed_at IS NULL AND \
           ((date_trunc('day', s.opened_at AT TIME ZONE st.timezone) + st.auto_close_time::time \
             + CASE WHEN st.auto_close_time::time <= (s.opened_at AT TIME ZONE st.timezone)::time THEN interval '1 day' ELSE interval '0 day' END) \
            AT TIME ZONE st.timezone) <= $1",
    )
    .bind(now)
    .fetch_all(pool)
    .await?;
    let mut n = 0;
    for (id, org) in due {
        if close_shift(pool, org, id, None, "Автозакрытие", None, true, now).await.is_ok() {
            n += 1;
        }
    }
    Ok(n)
}

/// «Отменить закрытие»: только последняя смена филиала.
async fn reopen(State(s): State<AppState>, ctx: AuthContext, Path(id): Path<Uuid>) -> AppResult<Json<ShiftView>> {
    ctx.require("shift.edit_data")?;
    let row: Option<(Uuid, Option<DateTime<Utc>>)> =
        sqlx::query_as("SELECT store_id, closed_at FROM shifts WHERE id=$1 AND organization_id=$2").bind(id).bind(ctx.org_id).fetch_optional(&s.pool).await?;
    let (store_id, closed_at) = row.ok_or_else(|| AppError::NotFound("Смена не найдена".into()))?;
    let Some(closed_at) = closed_at else {
        return Err(AppError::BadRequest("Смена не закрыта".into()));
    };
    let (newer,): (i64,) = sqlx::query_as("SELECT count(*) FROM shifts WHERE store_id=$1 AND opened_at > (SELECT opened_at FROM shifts WHERE id=$2)")
        .bind(store_id)
        .bind(id)
        .fetch_one(&s.pool)
        .await?;
    if newer > 0 {
        return Err(AppError::BadRequest("Отменить можно только закрытие последней смены".into()));
    }
    let mut tx = s.pool.begin().await?;
    sqlx::query("UPDATE shifts SET closed_at=NULL, closed_by=NULL, expected_cash=NULL, actual_cash=NULL, diff=NULL, auto_closed=false WHERE id=$1")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("UPDATE working_hours SET ended_at=NULL WHERE shift_id=$1 AND ended_at = $2")
        .bind(id)
        .bind(closed_at)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(Json(view_one(&s.pool, ctx.org_id, id).await?))
}

async fn open_shift_store(pool: &PgPool, org: Uuid, id: Uuid) -> AppResult<Uuid> {
    let row: Option<(Uuid, Option<DateTime<Utc>>)> =
        sqlx::query_as("SELECT store_id, closed_at FROM shifts WHERE id=$1 AND organization_id=$2").bind(id).bind(org).fetch_optional(pool).await?;
    let (store, closed) = row.ok_or_else(|| AppError::NotFound("Смена не найдена".into()))?;
    if closed.is_some() {
        return Err(AppError::BadRequest("Смена закрыта".into()));
    }
    Ok(store)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StaffDto {
    pub user_id: Uuid,
}

async fn staff_add(
    State(s): State<AppState>,
    ctx: AuthContext,
    Path(id): Path<Uuid>,
    Json(dto): Json<StaffDto>,
) -> AppResult<Json<ShiftView>> {
    ctx.require("shift.edit_staff")?;
    let store = open_shift_store(&s.pool, ctx.org_id, id).await?;
    let (n,): (i64,) = sqlx::query_as("SELECT count(*) FROM users WHERE id=$1 AND organization_id=$2 AND active").bind(dto.user_id).bind(ctx.org_id).fetch_one(&s.pool).await?;
    if n == 0 {
        return Err(AppError::BadRequest("Сотрудник не найден".into()));
    }
    let mut tx = s.pool.begin().await?;
    let res = sqlx::query("INSERT INTO shift_staff (shift_id, user_id) VALUES ($1,$2) ON CONFLICT DO NOTHING").bind(id).bind(dto.user_id).execute(&mut *tx).await?;
    if res.rows_affected() > 0 {
        start_hours(&mut tx, ctx.org_id, store, id, dto.user_id, ctx.user_id).await?;
    }
    tx.commit().await?;
    Ok(Json(view_one(&s.pool, ctx.org_id, id).await?))
}

async fn staff_remove(State(s): State<AppState>, ctx: AuthContext, Path((id, user_id)): Path<(Uuid, Uuid)>) -> AppResult<Json<ShiftView>> {
    ctx.require("shift.edit_staff")?;
    open_shift_store(&s.pool, ctx.org_id, id).await?;
    let mut tx = s.pool.begin().await?;
    sqlx::query("DELETE FROM shift_staff WHERE shift_id=$1 AND user_id=$2").bind(id).bind(user_id).execute(&mut *tx).await?;
    // сотрудник ушёл из смены: его часы завершаются сейчас
    sqlx::query("UPDATE working_hours SET ended_at=GREATEST(now(), started_at) WHERE shift_id=$1 AND user_id=$2 AND ended_at IS NULL")
        .bind(id)
        .bind(user_id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(Json(view_one(&s.pool, ctx.org_id, id).await?))
}

#[derive(Deserialize)]
pub struct CashboxDto {
    /// внесение > 0, изъятие < 0
    pub amount: f64,
    pub comment: Option<String>,
}

async fn cashbox(
    State(s): State<AppState>,
    ctx: AuthContext,
    Path(id): Path<Uuid>,
    Json(dto): Json<CashboxDto>,
) -> AppResult<Json<ShiftView>> {
    ctx.require("shift.view")?;
    if !dto.amount.is_finite() || dto.amount == 0.0 {
        return Err(AppError::BadRequest("Укажите сумму".into()));
    }
    open_shift_store(&s.pool, ctx.org_id, id).await?;
    sqlx::query("INSERT INTO cashbox_transactions (id, organization_id, shift_id, user_id, amount, comment) VALUES ($1,$2,$3,$4,$5::float8,$6)")
        .bind(Uuid::new_v4())
        .bind(ctx.org_id)
        .bind(id)
        .bind(ctx.user_id)
        .bind(dto.amount)
        .bind(dto.comment.unwrap_or_default())
        .execute(&s.pool)
        .await?;
    Ok(Json(view_one(&s.pool, ctx.org_id, id).await?))
}
