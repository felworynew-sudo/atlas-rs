//! /api/reports — отчёты: продажи, прибыль и убытки (P&L), товары, ревизии, статистика главной; экспорт CSV.
//! Период `[from, to)` задаётся в UTC (по умолчанию — последние 7 суток); дни группируются по часовому поясу филиала.
use crate::{
    auth::AuthContext,
    error::{AppError, AppResult},
    routes::sales::check_store,
    AppState,
};
use atlas_domain::{
    finance::{monthly_share, pnl, PnlInput},
    sale::round2,
};
use axum::{
    extract::{Query, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use chrono::{DateTime, Duration, NaiveDate, Utc};
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::PgPool;
use std::collections::{BTreeMap, HashMap};
use uuid::Uuid;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/sales", get(sales_report))
        .route("/profit", get(profit_report))
        .route("/products", get(products_report))
        .route("/revisions", get(revisions_report))
        .route("/home", get(home_stats))
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ReportQuery {
    pub store_id: Option<Uuid>,
    pub from: Option<DateTime<Utc>>,
    pub to: Option<DateTime<Utc>>,
    /// Период датами `YYYY-MM-DD` (включительно): границы суток считаются по часовому поясу филиала.
    /// Имеет приоритет над `from`/`to`.
    pub from_date: Option<NaiveDate>,
    pub to_date: Option<NaiveDate>,
    /// payment (по умолчанию) | purchase | user
    pub group_by: Option<String>,
    /// json (по умолчанию) | csv
    pub format: Option<String>,
}

struct Scope {
    stores: Vec<Uuid>,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
    /// первый и последний день периода (включительно) для документов с датой без времени
    from_day: NaiveDate,
    to_day: NaiveDate,
}

impl Scope {
    fn from_date(&self) -> NaiveDate {
        self.from_day
    }
    fn to_date(&self) -> NaiveDate {
        self.to_day
    }
}

/// Филиалы отчёта: выбранный или все доступные пользователю.
async fn scope(pool: &PgPool, ctx: &AuthContext, q: &ReportQuery) -> AppResult<Scope> {
    let stores = match q.store_id {
        Some(id) => {
            check_store(pool, ctx, id).await?;
            vec![id]
        }
        None => {
            if ctx.is_admin() || ctx.can("settings.other_stores") {
                let rows: Vec<(Uuid,)> = sqlx::query_as("SELECT id FROM stores WHERE organization_id=$1 AND deleted_at IS NULL")
                    .bind(ctx.org_id)
                    .fetch_all(pool)
                    .await?;
                rows.into_iter().map(|r| r.0).collect()
            } else {
                let (ids,): (Vec<Uuid>,) = sqlx::query_as("SELECT store_ids FROM users WHERE id=$1")
                    .bind(ctx.user_id)
                    .fetch_one(pool)
                    .await?;
                ids
            }
        }
    };
    if let (Some(fd), Some(td)) = (q.from_date, q.to_date) {
        if td < fd {
            return Err(AppError::BadRequest("Начало периода должно быть раньше конца".into()));
        }
        // сутки `[fd 00:00, td+1 00:00)` по часовому поясу первого филиала отчёта
        let tz: String = match stores.first() {
            Some(id) => sqlx::query_as::<_, (String,)>("SELECT timezone FROM stores WHERE id=$1").bind(id).fetch_one(pool).await?.0,
            None => "UTC".into(),
        };
        let (from, to): (DateTime<Utc>, DateTime<Utc>) =
            sqlx::query_as("SELECT ($1::date)::timestamp AT TIME ZONE $3::text, (($2::date + 1))::timestamp AT TIME ZONE $3::text")
                .bind(fd)
                .bind(td)
                .bind(&tz)
                .fetch_one(pool)
                .await?;
        return Ok(Scope { stores, from, to, from_day: fd, to_day: td });
    }
    let to = q.to.unwrap_or_else(|| Utc::now() + Duration::days(1));
    let from = q.from.unwrap_or(to - Duration::days(7));
    if from >= to {
        return Err(AppError::BadRequest("Начало периода должно быть раньше конца".into()));
    }
    let (from_day, to_day) = (from.date_naive(), (to - Duration::seconds(1)).date_naive());
    Ok(Scope { stores, from, to, from_day, to_day })
}

/// Таблица отчёта в нужном формате: `xlsx` — книга Excel (числа записываются числами), иначе CSV.
pub fn table_response(name: &str, rows: Vec<Vec<String>>, format: Option<&str>) -> Response {
    if format == Some("xlsx") {
        if let Some(r) = xlsx_response(name, &rows) {
            return r;
        }
    }
    csv_response(name, rows)
}

fn xlsx_response(name: &str, rows: &[Vec<String>]) -> Option<Response> {
    use rust_xlsxwriter::{Format, Workbook};
    let mut book = Workbook::new();
    let sheet = book.add_worksheet();
    let bold = Format::new().set_bold();
    for (r, row) in rows.iter().enumerate() {
        for (c, cell) in row.iter().enumerate() {
            let (r, c) = (r as u32, c as u16);
            let done = if r == 0 {
                sheet.write_string_with_format(r, c, cell, &bold).map(|_| ())
            } else if let Some(v) = cell.trim().parse::<f64>().ok().filter(|v| v.is_finite() && !cell.trim().is_empty() && !cell.starts_with('+')) {
                sheet.write_number(r, c, v).map(|_| ())
            } else {
                sheet.write_string(r, c, cell).map(|_| ())
            };
            done.ok()?;
        }
    }
    sheet.autofit();
    let bytes = book.save_to_buffer().ok()?;
    Some(
        (
            StatusCode::OK,
            [
                (header::CONTENT_TYPE, "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet".to_string()),
                (header::CONTENT_DISPOSITION, format!("attachment; filename=\"{name}.xlsx\"")),
            ],
            bytes,
        )
            .into_response(),
    )
}

pub fn csv_response(name: &str, rows: Vec<Vec<String>>) -> Response {
    let esc = |f: &str| {
        if f.contains([',', '"', '\n', ';']) {
            format!("\"{}\"", f.replace('"', "\"\""))
        } else {
            f.to_string()
        }
    };
    let mut body = String::from("\u{feff}");
    for r in rows {
        body.push_str(&r.iter().map(|f| esc(f)).collect::<Vec<_>>().join(","));
        body.push_str("\r\n");
    }
    (
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, "text/csv; charset=utf-8".to_string()),
            (header::CONTENT_DISPOSITION, format!("attachment; filename=\"{name}.csv\"")),
        ],
        body,
    )
        .into_response()
}

fn n(v: f64) -> String {
    format!("{:.2}", v)
}

fn wants_csv(q: &ReportQuery) -> bool {
    matches!(q.format.as_deref(), Some("csv" | "xlsx"))
}

// ---------- общие выборки ----------
struct SaleRow {
    total: f64,
    bonus_reduction: f64,
    cost_total: f64,
    payment: Option<Uuid>,
    second_payment: Option<Uuid>,
    first_value: Option<f64>,
    second_value: Option<f64>,
    purchase: Option<Uuid>,
    user: Uuid,
    day: NaiveDate,
}

async fn sale_rows(pool: &PgPool, org: Uuid, sc: &Scope) -> AppResult<Vec<SaleRow>> {
    let rows: Vec<(f64, f64, f64, Option<Uuid>, Option<Uuid>, Option<f64>, Option<f64>, Option<Uuid>, Uuid, NaiveDate)> =
        sqlx::query_as(
            "SELECT s.total::float8, s.bonus_reduction::float8, s.cost_total::float8, s.payment_method_id, s.second_payment_method_id, \
                    s.first_payment_value::float8, s.second_payment_value::float8, s.purchase_method_id, s.user_id, \
                    (s.sale_date AT TIME ZONE st.timezone)::date \
             FROM sales s JOIN stores st ON st.id = s.store_id \
             WHERE s.organization_id=$1 AND s.deleted_at IS NULL AND s.store_id = ANY($2) AND s.sale_date >= $3 AND s.sale_date < $4",
        )
        .bind(org)
        .bind(&sc.stores)
        .bind(sc.from)
        .bind(sc.to)
        .fetch_all(pool)
        .await?;
    Ok(rows
        .into_iter()
        .map(|r| SaleRow {
            total: r.0,
            bonus_reduction: r.1,
            cost_total: r.2,
            payment: r.3,
            second_payment: r.4,
            first_value: r.5,
            second_value: r.6,
            purchase: r.7,
            user: r.8,
            day: r.9,
        })
        .collect())
}

struct ReturnRow {
    amount: f64,
    payment: Option<Uuid>,
    purchase: Option<Uuid>,
    user: Uuid,
}

async fn return_rows(pool: &PgPool, org: Uuid, sc: &Scope) -> AppResult<Vec<ReturnRow>> {
    let rows: Vec<(f64, Option<Uuid>, Option<Uuid>, Uuid)> = sqlx::query_as(
        "SELECT r.amount::float8, s.payment_method_id, s.purchase_method_id, s.user_id \
         FROM sale_returns r JOIN sales s ON s.id = r.sale_id \
         WHERE r.organization_id=$1 AND s.deleted_at IS NULL AND s.store_id = ANY($2) AND r.created_at >= $3 AND r.created_at < $4",
    )
    .bind(org)
    .bind(&sc.stores)
    .bind(sc.from)
    .bind(sc.to)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(|r| ReturnRow { amount: r.0, payment: r.1, purchase: r.2, user: r.3 }).collect())
}

async fn names(pool: &PgPool, org: Uuid) -> AppResult<HashMap<Uuid, String>> {
    let mut map: HashMap<Uuid, String> = HashMap::new();
    let items: Vec<(Uuid, String)> = sqlx::query_as("SELECT id, name FROM list_items WHERE organization_id=$1").bind(org).fetch_all(pool).await?;
    map.extend(items);
    let users: Vec<(Uuid, String)> = sqlx::query_as("SELECT id, name FROM users WHERE organization_id=$1").bind(org).fetch_all(pool).await?;
    map.extend(users);
    Ok(map)
}

// =========================== продажи ===========================
#[derive(Default)]
struct Group {
    sales: f64,
    count: i64,
    returns: f64,
    returns_count: i64,
}

async fn sales_report(State(s): State<AppState>, ctx: AuthContext, Query(q): Query<ReportQuery>) -> AppResult<Response> {
    ctx.require("reports.view")?;
    let sc = scope(&s.pool, &ctx, &q).await?;
    let rows = sale_rows(&s.pool, ctx.org_id, &sc).await?;
    let returns = return_rows(&s.pool, ctx.org_id, &sc).await?;
    let nm = names(&s.pool, ctx.org_id).await?;
    let by = q.group_by.as_deref().unwrap_or("payment");
    if !["payment", "purchase", "user"].contains(&by) {
        return Err(AppError::BadRequest("Группировка: payment, purchase или user".into()));
    }

    let mut groups: HashMap<Option<Uuid>, Group> = HashMap::new();
    let mut days: BTreeMap<NaiveDate, (f64, i64)> = BTreeMap::new();
    let (mut total, mut count) = (0.0, 0i64);
    for r in &rows {
        total += r.total;
        count += 1;
        let d = days.entry(r.day).or_default();
        d.0 += r.total;
        d.1 += 1;
        match by {
            "purchase" => {
                let g = groups.entry(r.purchase).or_default();
                g.sales += r.total;
                g.count += 1;
            }
            "user" => {
                let g = groups.entry(Some(r.user)).or_default();
                g.sales += r.total;
                g.count += 1;
            }
            _ => match r.second_payment {
                // смешанная оплата делится между методами по суммам платежей
                Some(second) => {
                    let g = groups.entry(r.payment).or_default();
                    g.sales += r.first_value.unwrap_or(0.0);
                    g.count += 1;
                    let g2 = groups.entry(Some(second)).or_default();
                    g2.sales += r.second_value.unwrap_or(0.0);
                    g2.count += 1;
                }
                None => {
                    let g = groups.entry(r.payment).or_default();
                    g.sales += r.total;
                    g.count += 1;
                }
            },
        }
    }
    let (mut ret_total, mut ret_count) = (0.0, 0i64);
    for r in &returns {
        ret_total += r.amount;
        ret_count += 1;
        let key = match by {
            "purchase" => r.purchase,
            "user" => Some(r.user),
            _ => r.payment,
        };
        let g = groups.entry(key).or_default();
        g.returns += r.amount;
        g.returns_count += 1;
    }

    let mut group_list: Vec<(String, Option<Uuid>, Group)> = groups
        .into_iter()
        .map(|(k, g)| (k.and_then(|id| nm.get(&id).cloned()).unwrap_or_else(|| "Не указано".into()), k, g))
        .collect();
    group_list.sort_by(|a, b| a.0.cmp(&b.0));

    if wants_csv(&q) {
        let mut out = vec![vec!["Группа".into(), "Продажи".into(), "Кол-во продаж".into(), "Возвраты".into(), "Кол-во возвратов".into()]];
        for (name, _, g) in &group_list {
            out.push(vec![name.clone(), n(g.sales), g.count.to_string(), n(g.returns), g.returns_count.to_string()]);
        }
        out.push(vec![]);
        out.push(vec!["День".into(), "Продажи".into(), "Кол-во продаж".into()]);
        for (d, (sum, c)) in &days {
            out.push(vec![d.to_string(), n(*sum), c.to_string()]);
        }
        return Ok(table_response("sales-report", out, q.format.as_deref()));
    }
    Ok(Json(json!({
        "totals": {
            "sales": round2(total), "count": count,
            "avgCheck": if count > 0 { round2(total / count as f64) } else { 0.0 },
            "returns": round2(ret_total), "returnsCount": ret_count,
        },
        "groups": group_list.iter().map(|(name, id, g)| json!({
            "id": id, "name": name, "sales": round2(g.sales), "count": g.count,
            "returns": round2(g.returns), "returnsCount": g.returns_count,
        })).collect::<Vec<_>>(),
        "days": days.iter().map(|(d, (sum, c))| json!({"date": d, "sales": round2(*sum), "count": c})).collect::<Vec<_>>(),
    }))
    .into_response())
}

// =========================== P&L ===========================
async fn profit_report(State(s): State<AppState>, ctx: AuthContext, Query(q): Query<ReportQuery>) -> AppResult<Response> {
    ctx.require("reports.view")?;
    let sc = scope(&s.pool, &ctx, &q).await?;
    let org = ctx.org_id;
    let rows = sale_rows(&s.pool, org, &sc).await?;
    let returns = return_rows(&s.pool, org, &sc).await?;

    // комиссия банка: процент метода оплаты от суммы платежа
    let commissions: Vec<(Uuid, f64)> = sqlx::query_as(
        "SELECT id, COALESCE((params->>'commission')::float8, 0) FROM list_items WHERE organization_id=$1 AND kind='payment_methods'",
    )
    .bind(org)
    .fetch_all(&s.pool)
    .await?;
    let comm: HashMap<Uuid, f64> = commissions.into_iter().collect();
    let pct = |id: Option<Uuid>| id.and_then(|i| comm.get(&i).copied()).unwrap_or(0.0);

    let (mut sales, mut cost, mut bonus, mut commission) = (0.0, 0.0, 0.0, 0.0);
    for r in &rows {
        sales += r.total + r.bonus_reduction;
        cost += r.cost_total;
        bonus += r.bonus_reduction;
        match r.second_payment {
            Some(second) => {
                commission += r.first_value.unwrap_or(0.0) * pct(r.payment) / 100.0;
                commission += r.second_value.unwrap_or(0.0) * pct(Some(second)) / 100.0;
            }
            None => commission += r.total * pct(r.payment) / 100.0,
        }
    }
    let returns_sum: f64 = returns.iter().map(|r| r.amount).sum();
    // завершённые комплексные продажи: выручка — стоимость для клиента, себестоимость — расходы по списку
    let (cs_sales, cs_cost): (f64, f64) = sqlx::query_as(
        "SELECT COALESCE(SUM(c.final_price),0)::float8, \
                COALESCE(SUM((SELECT COALESCE(SUM(i.qty * i.cost + i.extra_cost),0) FROM complex_sale_items i WHERE i.sale_id = c.id)),0)::float8 \
         FROM complex_sales c WHERE c.organization_id=$1 AND c.deleted_at IS NULL AND c.status='done' AND c.store_id = ANY($2) \
           AND c.sale_date >= $3 AND c.sale_date < $4",
    )
    .bind(org)
    .bind(&sc.stores)
    .bind(sc.from)
    .bind(sc.to)
    .fetch_one(&s.pool)
    .await?;
    let sales = sales + cs_sales;
    let cost = cost + cs_cost;
    // товар, возвращённый на склад, возвращает и свою себестоимость; списанный возврат её не возвращает
    let (restocked_cost,): (f64,) = sqlx::query_as(
        "SELECT COALESCE(SUM(ri.qty * si.cost_price),0)::float8 FROM sale_return_items ri \
         JOIN sale_returns r ON r.id = ri.return_id JOIN sale_items si ON si.id = ri.sale_item_id JOIN sales s ON s.id = r.sale_id \
         WHERE r.organization_id=$1 AND r.restock AND s.deleted_at IS NULL AND s.store_id = ANY($2) \
           AND r.created_at >= $3 AND r.created_at < $4",
    )
    .bind(org)
    .bind(&sc.stores)
    .bind(sc.from)
    .bind(sc.to)
    .fetch_one(&s.pool)
    .await?;
    let cost = cost - restocked_cost;

    let (writeoffs,): (f64,) = sqlx::query_as(
        "SELECT COALESCE(SUM(amount),0)::float8 FROM writeoffs WHERE organization_id=$1 AND deleted_at IS NULL \
         AND store_id = ANY($2) AND writeoff_date >= $3 AND writeoff_date < $4",
    )
    .bind(org)
    .bind(&sc.stores)
    .bind(sc.from)
    .bind(sc.to)
    .fetch_one(&s.pool)
    .await?;
    let (revision_diff,): (f64,) = sqlx::query_as(
        "SELECT COALESCE(SUM(total_diff),0)::float8 FROM revisions WHERE organization_id=$1 AND deleted_at IS NULL \
         AND store_id = ANY($2) AND revision_date >= $3 AND revision_date < $4",
    )
    .bind(org)
    .bind(&sc.stores)
    .bind(sc.from)
    .bind(sc.to)
    .fetch_one(&s.pool)
    .await?;
    let (supplies,): (f64,) = sqlx::query_as(
        "SELECT COALESCE(SUM(CASE WHEN has_return THEN -total ELSE total END),0)::float8 FROM supplies \
         WHERE organization_id=$1 AND deleted_at IS NULL AND store_id = ANY($2) AND supply_date >= $3 AND supply_date <= $4",
    )
    .bind(org)
    .bind(&sc.stores)
    .bind(sc.from_date())
    .bind(sc.to_date())
    .fetch_one(&s.pool)
    .await?;

    // справочные строки: перемещения (приход минус уход относительно выбранных филиалов) и стоимость остатков на начало и конец периода
    let (transfers,): (f64,) = sqlx::query_as(
        "SELECT COALESCE(SUM(CASE WHEN to_store_id = ANY($2) AND NOT from_store_id = ANY($2) THEN amount \
                                  WHEN from_store_id = ANY($2) AND NOT to_store_id = ANY($2) THEN -amount ELSE 0 END),0)::float8 \
         FROM transfers WHERE organization_id=$1 AND deleted_at IS NULL AND transfer_date >= $3 AND transfer_date < $4",
    )
    .bind(org)
    .bind(&sc.stores)
    .bind(sc.from)
    .bind(sc.to)
    .fetch_one(&s.pool)
    .await?;
    let costs = crate::routes::products::cost_nodes(&s.pool, org).await?;
    let stock_value = |at: DateTime<Utc>| {
        let pool = s.pool.clone();
        let stores = sc.stores.clone();
        async move {
            let rows: Vec<(Uuid, f64)> = sqlx::query_as(
                "SELECT product_id, SUM(delta)::float8 FROM product_movements WHERE organization_id=$1 AND store_id = ANY($2) AND created_at < $3 \
                 GROUP BY product_id HAVING SUM(delta) > 0",
            )
            .bind(org)
            .bind(&stores)
            .bind(at)
            .fetch_all(&pool)
            .await?;
            Ok::<_, AppError>(rows)
        }
    };
    let start_rows = stock_value(sc.from).await?;
    let end_rows = stock_value(sc.to).await?;
    let value_of = |rows: &[(Uuid, f64)]| -> f64 { rows.iter().map(|(pid, qty)| qty * atlas_domain::catalog::unit_cost(&costs, *pid)).sum() };
    let stock_start = value_of(&start_rows);
    let stock_end = value_of(&end_rows);
    // остатки с неизвестной закупочной ценой делают оценку стоимости неполной
    let unknown_stock = end_rows.iter().filter(|(pid, _)| atlas_domain::catalog::unit_cost(&costs, *pid) <= 0.0).count();

    let by_category: Vec<(Option<String>, f64)> = sqlx::query_as(
        "SELECT c.name, SUM(sp.amount)::float8 FROM spendings sp LEFT JOIN list_items c ON c.id = sp.category_id \
         WHERE sp.organization_id=$1 AND sp.deleted_at IS NULL AND sp.kind='operating' AND sp.store_id = ANY($2) \
           AND sp.spending_date >= $3 AND sp.spending_date <= $4 GROUP BY c.name ORDER BY c.name",
    )
    .bind(org)
    .bind(&sc.stores)
    .bind(sc.from_date())
    .bind(sc.to_date())
    .fetch_all(&s.pool)
    .await?;
    let operating: f64 = by_category.iter().map(|r| r.1).sum();
    let (capital,): (f64,) = sqlx::query_as(
        "SELECT COALESCE(SUM(amount),0)::float8 FROM spendings WHERE organization_id=$1 AND deleted_at IS NULL AND kind='capital' \
         AND store_id = ANY($2) AND spending_date >= $3 AND spending_date <= $4",
    )
    .bind(org)
    .bind(&sc.stores)
    .bind(sc.from_date())
    .bind(sc.to_date())
    .fetch_one(&s.pool)
    .await?;

    let recurring: Vec<(String, f64, NaiveDate, Option<NaiveDate>)> = sqlx::query_as(
        "SELECT name, amount::float8, start_date, end_date FROM recurring_expenses \
         WHERE organization_id=$1 AND deleted_at IS NULL AND store_id = ANY($2)",
    )
    .bind(org)
    .bind(&sc.stores)
    .fetch_all(&s.pool)
    .await?;
    let monthly: Vec<(String, f64)> = recurring
        .into_iter()
        .map(|(name, amount, start, end)| (name, monthly_share(amount, start, end, sc.from_date(), sc.to_date())))
        .filter(|(_, v)| *v > 0.0)
        .collect();
    let monthly_total: f64 = monthly.iter().map(|m| m.1).sum();

    // разница закрытия смен (недосдача < 0, излишек > 0) за период
    let (shift_diff,): (f64,) = sqlx::query_as(
        "SELECT COALESCE(SUM(diff),0)::float8 FROM shifts WHERE organization_id=$1 AND store_id = ANY($2) AND closed_at >= $3 AND closed_at < $4",
    )
    .bind(org)
    .bind(&sc.stores)
    .bind(sc.from)
    .bind(sc.to)
    .fetch_one(&s.pool)
    .await?;
    let salaries = crate::routes::salary::total_accrued(&s.pool, org, sc.from, sc.to, &sc.stores).await?;

    let res = pnl(&PnlInput {
        sales,
        cost_of_sales: cost,
        paid_by_bonus: bonus,
        returns: returns_sum,
        writeoffs,
        revision_diff,
        shift_diff,
        salaries,
        bank_commission: commission,
        operating_expenses: operating,
        monthly_expenses: monthly_total,
        capital_expenses: capital,
    });

    if wants_csv(&q) {
        let mut out: Vec<Vec<String>> = vec![vec!["Показатель".into(), "Сумма".into()]];
        let mut line = |k: &str, v: f64| out.push(vec![k.to_string(), n(v)]);
        line("Продажи", sales);
        line("Себестоимость товаров", cost);
        line("Оплачено бонусами", bonus);
        line("Маржа", res.gross_income);
        line("Возвраты", returns_sum);
        line("Списания", writeoffs);
        line("Разница в ревизиях", revision_diff);
        line("Комиссия банка", commission);
        for (name, v) in &by_category {
            line(&format!("Расходы: {}", name.clone().unwrap_or_else(|| "Без категории".into())), *v);
        }
        for (name, v) in &monthly {
            line(&format!("Ежемесячные: {name}"), *v);
        }
        line("Операционная прибыль", res.operating_profit);
        line("Капитальные вложения", capital);
        line("Чистая прибыль", res.net_profit);
        line("Зарплаты", salaries);
        line("Разница в закрытиях смен", shift_diff);
        line("Приемки (справочно)", supplies);
        line("Перемещения (справочно)", transfers);
        line("Стоимость товаров на начало", stock_start);
        line("Стоимость товаров на конец", stock_end);
        line("Разница в стоимости", stock_end - stock_start);
        return Ok(table_response("profit-report", out, q.format.as_deref()));
    }
    Ok(Json(json!({
        "sales": round2(sales),
        "costOfSales": round2(cost),
        "paidByBonus": round2(bonus),
        "grossIncome": res.gross_income,
        "returns": round2(returns_sum),
        "writeoffs": round2(writeoffs),
        "revisionDiff": round2(revision_diff),
        "shiftDiff": round2(shift_diff),
        "salaries": salaries,
        "bankCommission": round2(commission),
        "expenses": { "total": round2(operating), "byCategory": by_category.iter().map(|(name, v)| json!({
            "name": name.clone().unwrap_or_else(|| "Без категории".into()), "amount": round2(*v) })).collect::<Vec<_>>() },
        "monthlyExpenses": { "total": round2(monthly_total), "items": monthly.iter().map(|(name, v)| json!({"name": name, "amount": v})).collect::<Vec<_>>() },
        "operatingProfit": res.operating_profit,
        "capitalExpenses": round2(capital),
        "netProfit": res.net_profit,
        "supplies": round2(supplies),
        "transfers": round2(transfers),
        "stockValueStart": round2(stock_start),
        "stockValueEnd": round2(stock_end),
        "stockValueDiff": round2(stock_end - stock_start),
        "unknownStockItems": unknown_stock,
        "checks": rows.len(),
    }))
    .into_response())
}

// =========================== товары / списания ===========================
#[derive(Default, Clone)]
struct ProductAgg {
    sold_qty: f64,
    sold_sum: f64,
    cost: f64,
    checks: i64,
    writeoff_qty: f64,
    writeoff_sum: f64,
    supply_qty: f64,
    supply_sum: f64,
}

impl ProductAgg {
    fn add(&mut self, o: &ProductAgg) {
        self.sold_qty += o.sold_qty;
        self.sold_sum += o.sold_sum;
        self.cost += o.cost;
        self.checks += o.checks;
        self.writeoff_qty += o.writeoff_qty;
        self.writeoff_sum += o.writeoff_sum;
        self.supply_qty += o.supply_qty;
        self.supply_sum += o.supply_sum;
    }
    fn json(&self, key: Value, name: &str) -> Value {
        let profit = self.sold_sum - self.cost;
        json!({
            "id": key, "name": name,
            "soldQty": round2(self.sold_qty), "soldSum": round2(self.sold_sum), "cost": round2(self.cost),
            "profit": round2(profit), "margin": if self.sold_sum > 0.0 { round2(profit / self.sold_sum * 100.0) } else { 0.0 },
            "checks": self.checks, "writeoffQty": round2(self.writeoff_qty), "writeoffSum": round2(self.writeoff_sum),
            "supplyQty": round2(self.supply_qty), "supplySum": round2(self.supply_sum),
        })
    }
}

async fn products_report(State(s): State<AppState>, ctx: AuthContext, Query(q): Query<ReportQuery>) -> AppResult<Response> {
    ctx.require("reports.view")?;
    let sc = scope(&s.pool, &ctx, &q).await?;
    let org = ctx.org_id;
    let mut agg: HashMap<Uuid, ProductAgg> = HashMap::new();

    let sold: Vec<(Uuid, f64, f64, f64, i64)> = sqlx::query_as(
        "SELECT si.product_id, SUM(si.qty - si.returned_qty)::float8, SUM((si.qty - si.returned_qty) * si.price)::float8, \
                SUM((si.qty - si.returned_qty) * si.cost_price)::float8, COUNT(DISTINCT s.id) \
         FROM sale_items si JOIN sales s ON s.id = si.sale_id \
         WHERE s.organization_id=$1 AND s.deleted_at IS NULL AND si.product_id IS NOT NULL AND s.store_id = ANY($2) \
           AND s.sale_date >= $3 AND s.sale_date < $4 GROUP BY si.product_id",
    )
    .bind(org)
    .bind(&sc.stores)
    .bind(sc.from)
    .bind(sc.to)
    .fetch_all(&s.pool)
    .await?;
    for (pid, qty, sum, cost, checks) in sold {
        let a = agg.entry(pid).or_default();
        a.sold_qty = qty;
        a.sold_sum = sum;
        a.cost = cost;
        a.checks = checks;
    }
    let wo: Vec<(Uuid, f64, f64)> = sqlx::query_as(
        "SELECT product_id, SUM(qty)::float8, SUM(amount)::float8 FROM writeoffs WHERE organization_id=$1 AND deleted_at IS NULL \
         AND store_id = ANY($2) AND writeoff_date >= $3 AND writeoff_date < $4 GROUP BY product_id",
    )
    .bind(org)
    .bind(&sc.stores)
    .bind(sc.from)
    .bind(sc.to)
    .fetch_all(&s.pool)
    .await?;
    for (pid, qty, sum) in wo {
        let a = agg.entry(pid).or_default();
        a.writeoff_qty = qty;
        a.writeoff_sum = sum;
    }
    let sup: Vec<(Uuid, f64, f64)> = sqlx::query_as(
        "SELECT i.product_id, SUM(CASE WHEN s.has_return THEN -i.qty ELSE i.qty END)::float8, \
                SUM(CASE WHEN s.has_return THEN -i.qty * i.price ELSE i.qty * i.price END)::float8 \
         FROM supply_items i JOIN supplies s ON s.id = i.supply_id \
         WHERE s.organization_id=$1 AND s.deleted_at IS NULL AND s.store_id = ANY($2) AND s.supply_date >= $3 AND s.supply_date <= $4 \
         GROUP BY i.product_id",
    )
    .bind(org)
    .bind(&sc.stores)
    .bind(sc.from_date())
    .bind(sc.to_date())
    .fetch_all(&s.pool)
    .await?;
    for (pid, qty, sum) in sup {
        let a = agg.entry(pid).or_default();
        a.supply_qty = qty;
        a.supply_sum = sum;
    }

    let prods: Vec<(Uuid, String, Option<Uuid>, Option<Uuid>)> =
        sqlx::query_as("SELECT id, name, category_id, supplier_id FROM products WHERE organization_id=$1").bind(org).fetch_all(&s.pool).await?;
    let cats: HashMap<Uuid, String> = sqlx::query_as::<_, (Uuid, String)>("SELECT id, name FROM product_categories WHERE organization_id=$1")
        .bind(org)
        .fetch_all(&s.pool)
        .await?
        .into_iter()
        .collect();
    let sups: HashMap<Uuid, String> = sqlx::query_as::<_, (Uuid, String)>("SELECT id, name FROM suppliers WHERE organization_id=$1")
        .bind(org)
        .fetch_all(&s.pool)
        .await?
        .into_iter()
        .collect();

    let mut by_cat: HashMap<Option<Uuid>, ProductAgg> = HashMap::new();
    let mut by_sup: HashMap<Option<Uuid>, ProductAgg> = HashMap::new();
    let mut items: Vec<(String, Uuid, ProductAgg)> = Vec::new();
    for (id, name, cat, sup) in prods {
        if let Some(a) = agg.get(&id) {
            by_cat.entry(cat).or_default().add(a);
            by_sup.entry(sup).or_default().add(a);
            items.push((name, id, a.clone()));
        }
    }
    items.sort_by(|a, b| b.2.sold_sum.partial_cmp(&a.2.sold_sum).unwrap_or(std::cmp::Ordering::Equal).then(a.0.cmp(&b.0)));
    let label = |m: &HashMap<Uuid, String>, k: Option<Uuid>, none: &str| k.and_then(|i| m.get(&i).cloned()).unwrap_or_else(|| none.into());
    let mut cat_rows: Vec<(String, Option<Uuid>, ProductAgg)> =
        by_cat.into_iter().map(|(k, a)| (label(&cats, k, "Без категории"), k, a)).collect();
    cat_rows.sort_by(|a, b| a.0.cmp(&b.0));
    let mut sup_rows: Vec<(String, Option<Uuid>, ProductAgg)> =
        by_sup.into_iter().map(|(k, a)| (label(&sups, k, "Без поставщика"), k, a)).collect();
    sup_rows.sort_by(|a, b| a.0.cmp(&b.0));

    if wants_csv(&q) {
        let mut out = vec![vec![
            "Товар".into(), "Продажи кол-во".into(), "Продажи сумма".into(), "Себестоимость".into(), "Прибыль".into(),
            "Списания кол-во".into(), "Списания сумма".into(), "Приемки кол-во".into(), "Приемки сумма".into(),
        ]];
        for (name, _, a) in &items {
            out.push(vec![
                name.clone(), n(a.sold_qty), n(a.sold_sum), n(a.cost), n(a.sold_sum - a.cost),
                n(a.writeoff_qty), n(a.writeoff_sum), n(a.supply_qty), n(a.supply_sum),
            ]);
        }
        return Ok(table_response("products-report", out, q.format.as_deref()));
    }
    Ok(Json(json!({
        "byCategory": cat_rows.iter().map(|(name, id, a)| a.json(json!(id), name)).collect::<Vec<_>>(),
        "bySupplier": sup_rows.iter().map(|(name, id, a)| a.json(json!(id), name)).collect::<Vec<_>>(),
        "byProduct": items.iter().map(|(name, id, a)| a.json(json!(id), name)).collect::<Vec<_>>(),
    }))
    .into_response())
}

// =========================== ревизии ===========================
async fn revisions_report(State(s): State<AppState>, ctx: AuthContext, Query(q): Query<ReportQuery>) -> AppResult<Response> {
    ctx.require("reports.view")?;
    let sc = scope(&s.pool, &ctx, &q).await?;
    let rows: Vec<(String, f64, f64, f64, f64)> = sqlx::query_as(
        "SELECT p.name, i.system_qty::float8, i.fact_qty::float8, i.diff_amount::float8, \
                ((i.fact_qty - i.system_qty) * COALESCE(ps.sale_price, 0))::float8 \
         FROM revision_items i JOIN revisions r ON r.id = i.revision_id JOIN products p ON p.id = i.product_id \
         LEFT JOIN product_stores ps ON ps.product_id = p.id AND ps.store_id = r.store_id \
         WHERE r.organization_id=$1 AND r.deleted_at IS NULL AND r.store_id = ANY($2) AND r.revision_date >= $3 AND r.revision_date < $4 \
           AND i.fact_qty <> i.system_qty ORDER BY p.name, r.revision_date",
    )
    .bind(ctx.org_id)
    .bind(&sc.stores)
    .bind(sc.from)
    .bind(sc.to)
    .fetch_all(&s.pool)
    .await?;
    let row = |r: &(String, f64, f64, f64, f64)| json!({"name": r.0, "was": r.1, "became": r.2, "diffAtPurchase": r.3, "diffAtSale": r.4});
    let minus: Vec<_> = rows.iter().filter(|r| r.3 < 0.0).collect();
    let plus: Vec<_> = rows.iter().filter(|r| r.3 > 0.0).collect();
    let minus_sum: f64 = minus.iter().map(|r| r.3).sum();
    let plus_sum: f64 = plus.iter().map(|r| r.3).sum();

    if wants_csv(&q) {
        let mut out = vec![vec!["Товар".into(), "Было".into(), "Стало".into(), "Разница по закупке".into(), "Разница по продаже".into()]];
        for r in &rows {
            out.push(vec![r.0.clone(), n(r.1), n(r.2), n(r.3), n(r.4)]);
        }
        return Ok(table_response("revisions-report", out, q.format.as_deref()));
    }
    Ok(Json(json!({
        "minus": round2(minus_sum), "plus": round2(plus_sum), "balance": round2(minus_sum + plus_sum),
        "minusList": minus.iter().map(|r| row(r)).collect::<Vec<_>>(),
        "plusList": plus.iter().map(|r| row(r)).collect::<Vec<_>>(),
    }))
    .into_response())
}

// =========================== главная ===========================
async fn home_stats(State(s): State<AppState>, ctx: AuthContext, Query(q): Query<ReportQuery>) -> AppResult<Json<Value>> {
    ctx.require("reports.home_stats")?;
    let base = scope(&s.pool, &ctx, &ReportQuery { store_id: q.store_id, ..Default::default() }).await?;
    let today = Utc::now().date_naive().and_hms_opt(0, 0, 0).expect("midnight").and_utc();
    let mut out = serde_json::Map::new();
    for (key, shift) in [("today", 0), ("weekAgo", 7)] {
        let from = today - Duration::days(shift);
        let sc = Scope { stores: base.stores.clone(), from, to: from + Duration::days(1), from_day: from.date_naive(), to_day: from.date_naive() };
        let rows = sale_rows(&s.pool, ctx.org_id, &sc).await?;
        let total: f64 = rows.iter().map(|r| r.total).sum();
        let count = rows.len();
        out.insert(
            key.into(),
            json!({"sales": round2(total), "checks": count, "avgCheck": if count > 0 { round2(total / count as f64) } else { 0.0 }}),
        );
    }
    Ok(Json(Value::Object(out)))
}
