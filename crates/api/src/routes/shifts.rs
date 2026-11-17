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
use sqlx::{FromRow, PgConnection, PgPool};
use uuid::Uuid;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list))
        .route("/current", get(current))
        .route("/open", post(open))
        .route("/{id}", get(one).patch(edit_closed))
        .route("/{id}/opening-cash", axum::routing::patch(edit_opening_cash))
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
    pub revision: String,
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
    /// закрытие по способам оплаты: [{paymentMethodId, expected, actual, diff}]
    pub close_details: Option<Value>,
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
    /// блок «по способу оплаты»: продажи и возвраты каждого способа
    pub by_method: Vec<MethodTotals>,
    /// «Продажи по сотрудникам»
    pub by_user: Vec<UserTotals>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MethodTotals {
    pub payment_method_id: Uuid,
    pub name: String,
    pub kind: String,
    pub sales: f64,
    pub returns: f64,
}

#[derive(Serialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct UserTotals {
    pub user_id: Uuid,
    pub name: String,
    pub sales: f64,
    pub checks: i64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShiftView {
    #[serde(flatten)]
    pub shift: ShiftRow,
    pub opened_by_name: String,
    pub opened_by_staff_id: Option<String>,
    pub staff: Vec<StaffView>,
    pub cashbox: Vec<CashboxView>,
    pub totals: ShiftTotals,
}

const SELECT_SHIFTS: &str = "SELECT id, xmin::text AS revision, store_id, opened_by, opened_at, closed_at, closed_by, opening_cash::float8 AS opening_cash, \
     expected_cash::float8 AS expected_cash, actual_cash::float8 AS actual_cash, diff::float8 AS diff, comment, auto_closed, close_details \
     FROM shifts WHERE organization_id=$1 AND ($2::uuid IS NULL OR id=$2) AND ($3::uuid IS NULL OR store_id=$3) \
       AND ($4::timestamptz IS NULL OR opened_at >= $4) AND ($5::timestamptz IS NULL OR opened_at < $5) \
       AND (NOT $6 OR closed_at IS NULL) ORDER BY opened_at DESC LIMIT 500";

/// Итоги смены за окно `[opened_at, end)`. Для открытой смены `end` — текущий момент.
async fn totals(db: &mut PgConnection, org: Uuid, shift: &ShiftRow, end: DateTime<Utc>) -> AppResult<ShiftTotals> {
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
    .fetch_one(&mut *db)
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
    .fetch_one(&mut *db)
    .await?;
    let (cash_spendings,): (f64,) = sqlx::query_as(
        "SELECT COALESCE(SUM(sp.amount),0)::float8 FROM spendings sp JOIN wallets w ON w.id = sp.wallet_id \
         WHERE sp.store_id=$1 AND sp.deleted_at IS NULL AND w.is_cash_register AND sp.created_at >= $2 AND sp.created_at < $3",
    )
    .bind(shift.store_id)
    .bind(shift.opened_at)
    .bind(end)
    .fetch_one(&mut *db)
    .await?;
    let (paid_supplies,): (f64,) = sqlx::query_as(
        "SELECT COALESCE(SUM(-t.delta),0)::float8 FROM wallet_transactions t JOIN wallets w ON w.id = t.wallet_id \
         WHERE t.kind='supply_payment' AND w.is_cash_register AND w.store_id=$1 AND t.created_at >= $2 AND t.created_at < $3",
    )
    .bind(shift.store_id)
    .bind(shift.opened_at)
    .bind(end)
    .fetch_one(&mut *db)
    .await?;
    let (deposits, withdrawals): (f64, f64) = sqlx::query_as(
        "SELECT COALESCE(SUM(amount) FILTER (WHERE amount > 0),0)::float8, COALESCE(-SUM(amount) FILTER (WHERE amount < 0),0)::float8 \
         FROM cashbox_transactions WHERE shift_id=$1",
    )
    .bind(shift.id)
    .fetch_one(&mut *db)
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
    // по способам оплаты (смешанная оплата делится по суммам платежей; возврат относится к основному способу продажи)
    let methods: Vec<(Uuid, String, String)> = sqlx::query_as(
        "SELECT id, name, COALESCE(params->>'kind','other') FROM list_items \
         WHERE organization_id=$1 AND kind='payment_methods' AND deleted_at IS NULL ORDER BY sort_order, created_at",
    )
    .bind(org)
    .fetch_all(&mut *db)
    .await?;
    let sale_rows: Vec<(Option<Uuid>, Option<Uuid>, f64, Option<f64>, Option<f64>)> = sqlx::query_as(
        "SELECT payment_method_id, second_payment_method_id, total::float8, first_payment_value::float8, second_payment_value::float8 \
         FROM sales WHERE shift_id=$1 AND deleted_at IS NULL",
    )
    .bind(shift.id)
    .fetch_all(&mut *db)
    .await?;
    let ret_rows: Vec<(Option<Uuid>, f64)> = sqlx::query_as(
        "SELECT s.payment_method_id, r.amount::float8 FROM sale_returns r JOIN sales s ON s.id = r.sale_id \
         WHERE s.store_id=$1 AND r.return_type='money' AND r.created_at >= $2 AND r.created_at < $3",
    )
    .bind(shift.store_id)
    .bind(shift.opened_at)
    .bind(end)
    .fetch_all(&mut *db)
    .await?;
    let mut by_method: Vec<MethodTotals> = methods
        .into_iter()
        .map(|(id, name, kind)| MethodTotals { payment_method_id: id, name, kind, sales: 0.0, returns: 0.0 })
        .collect();
    for (m1, m2, total, v1, v2) in &sale_rows {
        let parts: Vec<(Option<Uuid>, f64)> = match m2 {
            Some(second) => vec![(*m1, v1.unwrap_or(0.0)), (Some(*second), v2.unwrap_or(0.0))],
            None => vec![(*m1, *total)],
        };
        for (m, v) in parts {
            if let Some(t) = by_method.iter_mut().find(|t| Some(t.payment_method_id) == m) {
                t.sales += v;
            }
        }
    }
    for (m, amount) in &ret_rows {
        if let Some(t) = by_method.iter_mut().find(|t| Some(t.payment_method_id) == *m) {
            t.returns += amount;
        }
    }
    for t in &mut by_method {
        t.sales = round2(t.sales);
        t.returns = round2(t.returns);
    }
    let by_user = sqlx::query_as::<_, UserTotals>(
        "SELECT s.user_id, u.name, COALESCE(SUM(s.total),0)::float8 AS sales, COUNT(*) AS checks FROM sales s JOIN users u ON u.id = s.user_id \
         WHERE s.shift_id=$1 AND s.deleted_at IS NULL GROUP BY s.user_id, u.name ORDER BY u.name",
    )
    .bind(shift.id)
    .fetch_all(&mut *db)
    .await?;
    Ok(ShiftTotals {
        by_method,
        by_user,
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

async fn view(pool: &PgPool, org: Uuid, row: ShiftRow) -> AppResult<ShiftView> {
    let end = row.closed_at.unwrap_or_else(Utc::now);
    let t = totals(&mut *pool.acquire().await?, org, &row, end).await?;
    let opener: Option<(String, Option<String>)> = sqlx::query_as("SELECT name, staff_external_id FROM users WHERE id=$1")
        .bind(row.opened_by)
        .fetch_optional(pool)
        .await?;
    let (opened_by_name, opened_by_staff_id) = opener.unwrap_or_else(|| ("Сотрудник".into(), None));
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
    Ok(ShiftView { shift: row, opened_by_name, opened_by_staff_id, staff, cashbox, totals: t })
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
    view(pool, org, row).await
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
        out.push(view(&s.pool, ctx.org_id, r).await?);
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
        Some(r) => Ok(Json(serde_json::to_value(view(&s.pool, ctx.org_id, r).await?).map_err(|e| AppError::Internal(e.to_string()))?)),
        None => Ok(Json(Value::Null)),
    }
}

async fn one(State(s): State<AppState>, ctx: AuthContext, Path(id): Path<Uuid>) -> AppResult<Json<ShiftView>> {
    ctx.require("shift.view")?;
    let shift = view_one(&s.pool, ctx.org_id, id).await?;
    check_store(&s.pool, &ctx, shift.shift.store_id).await?;
    Ok(Json(shift))
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
    let id = open_shift(&s.pool, ctx.org_id, dto.store_id, ctx.user_id, dto.opening_cash, &dto.staff_ids).await?;
    Ok(Json(view_one(&s.pool, ctx.org_id, id).await?))
}

/// Открыть смену филиала: «Наличных в кассе на начало» по умолчанию — остаток счёта «Наличные с кассы»,
/// Открывший всегда в составе смены. У филиала сайта одна кассовая смена за календарный день.
async fn open_shift(pool: &PgPool, org: Uuid, store: Uuid, opener: Uuid, opening_cash: Option<f64>, staff_ids: &[Uuid]) -> AppResult<Uuid> {
    let mut tx = pool.begin().await?;
    let store_exists: Option<Uuid> = sqlx::query_scalar("SELECT id FROM stores WHERE id=$1 AND organization_id=$2 AND deleted_at IS NULL FOR UPDATE")
        .bind(store)
        .bind(org)
        .fetch_optional(&mut *tx)
        .await?;
    if store_exists.is_none() {
        return Err(AppError::BadRequest("Филиал не найден".into()));
    }
    let (open, today): (bool, bool) = sqlx::query_as(
        "SELECT EXISTS (SELECT 1 FROM shifts WHERE store_id=$1 AND closed_at IS NULL), \
                EXISTS (SELECT 1 FROM shifts sh JOIN stores st ON st.id=sh.store_id WHERE st.id=$1 AND st.external_id IS NOT NULL \
                    AND (sh.opened_at AT TIME ZONE st.timezone)::date=(now() AT TIME ZONE st.timezone)::date)",
    ).bind(store).fetch_one(&mut *tx).await?;
    if open {
        return Err(AppError::Conflict("Смена уже открыта".into()));
    }
    if today {
        return Err(AppError::Conflict("Смена на сегодня уже закрыта. Откройте её в истории смен, чтобы внести исправления".into()));
    }
    let opening = match opening_cash {
        Some(v) if v.is_finite() && v >= 0.0 => v,
        Some(_) => return Err(AppError::BadRequest("Наличных в кассе не может быть меньше нуля".into())),
        None => {
            let (v,): (f64,) = sqlx::query_as(
                "SELECT COALESCE(SUM(t.delta),0)::float8 FROM wallet_transactions t JOIN wallets w ON w.id = t.wallet_id                  WHERE w.is_cash_register AND w.store_id=$1 AND w.deleted_at IS NULL",
            )
            .bind(store)
            .fetch_one(&mut *tx)
            .await?;
            v.max(0.0)
        }
    };
    let mut staff = staff_ids.to_vec();
    if !staff.contains(&opener) {
        staff.push(opener);
    }
    let (found,): (i64,) = sqlx::query_as("SELECT count(*) FROM users WHERE organization_id=$1 AND active AND id = ANY($2)")
        .bind(org)
        .bind(&staff)
        .fetch_one(&mut *tx)
        .await?;
    if found as usize != staff.len() {
        return Err(AppError::BadRequest("Сотрудник не найден".into()));
    }
    let id = Uuid::new_v4();
    sqlx::query("INSERT INTO shifts (id, organization_id, store_id, opened_by, opening_cash) VALUES ($1,$2,$3,$4,$5::float8)")
        .bind(id)
        .bind(org)
        .bind(store)
        .bind(opener)
        .bind(opening)
        .execute(&mut *tx)
        .await?;
    for u in &staff {
        sqlx::query("INSERT INTO shift_staff (shift_id, user_id) VALUES ($1,$2)").bind(id).bind(u).execute(&mut *tx).await?;
        start_hours(&mut tx, org, store, id, *u, opener).await?;
    }
    tx.commit().await?;
    Ok(id)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloseDto {
    /// «Наличные — сумма на конец дня»
    pub actual_cash: f64,
    /// «QR / Терминал — сумма на конец дня» по каждому безналичному способу
    #[serde(default)]
    pub actual_by_method: Vec<MethodActual>,
    pub comment: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MethodActual {
    pub payment_method_id: Uuid,
    pub amount: f64,
}

async fn lock_shift(db: &mut PgConnection, org: Uuid, id: Uuid) -> AppResult<ShiftRow> {
    sqlx::query_as::<_, ShiftRow>(
        "SELECT id, xmin::text AS revision, store_id, opened_by, opened_at, closed_at, closed_by, opening_cash::float8, \
                expected_cash::float8, actual_cash::float8, diff::float8, comment, auto_closed, close_details \
         FROM shifts WHERE organization_id=$1 AND id=$2 FOR UPDATE",
    )
        .bind(org).bind(id)
        .fetch_optional(db).await?
        .ok_or_else(|| AppError::NotFound("Смена не найдена".into()))
}

fn close_values(t: &ShiftTotals, actual: f64, by_method: &[MethodActual]) -> AppResult<(f64, Option<Value>)> {
    if !actual.is_finite() || actual < 0.0 {
        return Err(AppError::BadRequest("Фактически в кассе: введите сумму".into()));
    }
    let mut diff = cash_difference(actual, t.expected_cash);
    // безналичные способы: ожидается «продажи − возвраты», разница суммируется в общую недосдачу
    let mut details: Vec<Value> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for m in by_method {
        if !seen.insert(m.payment_method_id) {
            return Err(AppError::BadRequest("Укажите одну сумму для каждого способа оплаты".into()));
        }
        let method = t
            .by_method
            .iter()
            .find(|x| x.payment_method_id == m.payment_method_id)
            .ok_or_else(|| AppError::BadRequest("Способ оплаты не найден".into()))?;
        if method.kind == "cash" || !(m.amount.is_finite() && m.amount >= 0.0) {
            return Err(AppError::BadRequest(format!("Некорректная сумма на конец дня: {}", method.name)));
        }
        let expected = round2(method.sales - method.returns);
        let d = cash_difference(m.amount, expected);
        diff = round2(diff + d);
        details.push(serde_json::json!({ "paymentMethodId": m.payment_method_id, "name": method.name, "expected": expected, "actual": m.amount, "diff": d }));
    }
    Ok((diff, if details.is_empty() { None } else { Some(Value::Array(details)) }))
}

/// Закрытие смены: расчёт наличных, разница «факт − расчёт», завершение рабочих часов.
async fn close_shift(pool: &PgPool, org: Uuid, id: Uuid, actual: Option<f64>, by_method: &[MethodActual], comment: &str, user: Option<Uuid>, auto: bool, now: DateTime<Utc>) -> AppResult<()> {
    let mut tx = pool.begin().await?;
    let row = lock_shift(&mut tx, org, id).await?;
    if row.closed_at.is_some() {
        return Err(AppError::BadRequest("Смена уже закрыта".into()));
    }
    let end = now.max(row.opened_at);
    let t = totals(&mut tx, org, &row, end).await?;
    let actual = actual.unwrap_or(t.expected_cash);
    let (diff, details) = close_values(&t, actual, by_method)?;
    let res = sqlx::query(
        "UPDATE shifts SET closed_at=$2, closed_by=$3, expected_cash=$4::float8, actual_cash=$5::float8, diff=$6::float8, comment=$7, auto_closed=$8, close_details=$9 \
         WHERE id=$1 AND closed_at IS NULL",
    )
    .bind(id)
    .bind(end)
    .bind(user)
    .bind(t.expected_cash)
    .bind(actual)
    .bind(diff)
    .bind(comment)
    .bind(auto)
    .bind(details)
    .execute(&mut *tx)
    .await?;
    if res.rows_affected() == 0 {
        return Err(AppError::BadRequest("Смена уже закрыта".into()));
    }
    sqlx::query("UPDATE working_hours SET ended_at=GREATEST($2, started_at) WHERE shift_id=$1 AND ended_at IS NULL")
        .bind(id)
        .bind(end)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(())
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct EditClosedDto {
    revision: String,
    opening_cash: f64,
    #[serde(flatten)]
    close: CloseDto,
}

async fn write_closed_totals(db: &mut PgConnection, org: Uuid, row: &ShiftRow, end: DateTime<Utc>, actual: f64, by_method: &[MethodActual], comment: &str) -> AppResult<()> {
    let t = totals(db, org, row, end).await?;
    let (diff, details) = close_values(&t, actual, by_method)?;
    sqlx::query("UPDATE shifts SET opening_cash=$2::float8, expected_cash=$3::float8, actual_cash=$4::float8, diff=$5::float8, close_details=$6, comment=$7 WHERE id=$1")
        .bind(row.id).bind(row.opening_cash).bind(t.expected_cash).bind(actual).bind(diff).bind(details).bind(comment)
        .execute(db).await?;
    Ok(())
}

async fn edit_closed(State(s): State<AppState>, ctx: AuthContext, Path(id): Path<Uuid>, Json(dto): Json<EditClosedDto>) -> AppResult<Json<ShiftView>> {
    ctx.require("shift.edit_data")?;
    if !dto.opening_cash.is_finite() || dto.opening_cash < 0.0 {
        return Err(AppError::BadRequest("Укажите сумму наличных на начало смены".into()));
    }
    let mut tx = s.pool.begin().await?;
    let mut row = lock_shift(&mut tx, ctx.org_id, id).await?;
    check_store(&s.pool, &ctx, row.store_id).await?;
    let end = row.closed_at.ok_or_else(|| AppError::BadRequest("Эта смена ещё открыта".into()))?;
    if row.revision != dto.revision {
        return Err(AppError::Conflict("Смена изменилась. Заново откройте её из истории перед сохранением".into()));
    }
    row.opening_cash = dto.opening_cash;
    write_closed_totals(&mut tx, ctx.org_id, &row, end, dto.close.actual_cash, &dto.close.actual_by_method,
        dto.close.comment.as_deref().unwrap_or(&row.comment).trim()).await?;
    tx.commit().await?;
    crate::events::log(&s.pool, ctx.org_id, Some(row.store_id), ctx.user_id, "shift_updated", "Исправлены данные закрытой смены").await;
    Ok(Json(view_one(&s.pool, ctx.org_id, id).await?))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct EditOpeningCashDto {
    revision: String,
    opening_cash: f64,
}

/// Исправление начальной налички открытой смены её открывшим сотрудником или администратором.
async fn edit_opening_cash(
    State(s): State<AppState>,
    ctx: AuthContext,
    Path(id): Path<Uuid>,
    Json(dto): Json<EditOpeningCashDto>,
) -> AppResult<Json<ShiftView>> {
    ctx.require("shift.view")?;
    if !dto.opening_cash.is_finite() || dto.opening_cash < 0.0 {
        return Err(AppError::BadRequest("Укажите сумму наличных на начало смены".into()));
    }
    let mut tx = s.pool.begin().await?;
    let row = lock_shift(&mut tx, ctx.org_id, id).await?;
    check_store(&s.pool, &ctx, row.store_id).await?;
    if row.closed_at.is_some() {
        return Err(AppError::BadRequest("Смена уже закрыта".into()));
    }
    if row.opened_by != ctx.user_id && !ctx.can("shift.edit_data") {
        return Err(AppError::Forbidden);
    }
    if row.revision != dto.revision {
        return Err(AppError::Conflict("Смена изменилась. Обновите страницу перед сохранением".into()));
    }
    sqlx::query("UPDATE shifts SET opening_cash=$2::float8 WHERE id=$1 AND closed_at IS NULL")
        .bind(id)
        .bind(round2(dto.opening_cash))
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    crate::events::log(&s.pool, ctx.org_id, Some(row.store_id), ctx.user_id, "shift_updated",
        &format!("Начальная наличка смены изменена: {} → {} ₽", crate::events::fmt(row.opening_cash), crate::events::fmt(round2(dto.opening_cash)))).await;
    Ok(Json(view_one(&s.pool, ctx.org_id, id).await?))
}

pub async fn refresh_closed_shift(db: &mut PgConnection, org: Uuid, id: Uuid) -> AppResult<()> {
    let row = lock_shift(db, org, id).await?;
    if let (Some(end), Some(actual)) = (row.closed_at, row.actual_cash) {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct RecordedActual { payment_method_id: Uuid, actual: f64 }
        let methods = match &row.close_details {
            Some(value) => serde_json::from_value::<Vec<RecordedActual>>(value.clone())
                .map_err(|e| AppError::Internal(e.to_string()))?,
            None => Vec::new(),
        };
        let by_method: Vec<MethodActual> = methods.into_iter()
            .map(|m| MethodActual { payment_method_id: m.payment_method_id, amount: m.actual }).collect();
        write_closed_totals(db, org, &row, end, actual, &by_method, &row.comment).await?;
    }
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
    close_shift(&s.pool, ctx.org_id, id, Some(dto.actual_cash), &dto.actual_by_method, dto.comment.as_deref().unwrap_or(""), Some(ctx.user_id), false, Utc::now()).await?;
    Ok(Json(view_one(&s.pool, ctx.org_id, id).await?))
}

/// Автозакрытие: смены, у которых наступило время автозакрытия филиала (в его часовом поясе). Возвращает число закрытых.
pub async fn auto_close_due(pool: &PgPool, now: DateTime<Utc>) -> AppResult<usize> {
    let due: Vec<(Uuid, Uuid, DateTime<Utc>)> = sqlx::query_as(
        "SELECT s.id, s.organization_id, deadline.at FROM shifts s JOIN stores st ON st.id = s.store_id \
         CROSS JOIN LATERAL (SELECT ((date_trunc('day', s.opened_at AT TIME ZONE st.timezone) + st.auto_close_time::time \
             + CASE WHEN st.auto_close_time::time <= (s.opened_at AT TIME ZONE st.timezone)::time THEN interval '1 day' ELSE interval '0 day' END) \
            AT TIME ZONE st.timezone) AS at) deadline WHERE s.closed_at IS NULL AND deadline.at <= $1",
    )
    .bind(now)
    .fetch_all(pool)
    .await?;
    let mut n = 0;
    for (id, org, end) in due {
        if close_shift(pool, org, id, None, &[], "Автозакрытие", None, true, end).await.is_ok() {
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
    sqlx::query("UPDATE shifts SET closed_at=NULL, closed_by=NULL, expected_cash=NULL, actual_cash=NULL, diff=NULL, auto_closed=false, close_details=NULL WHERE id=$1")
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
