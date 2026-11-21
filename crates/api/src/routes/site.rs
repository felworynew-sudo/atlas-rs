//! Связь с сайтом студии (дев-панель). Все методы вызывает только серверная часть сайта с заголовком
//! `X-Sso-Secret` (тот же секрет, что и для входа с сайта).
//!
//! - `POST /api/site/sync` — сайт присылает свои филиалы и сотрудников; Атлас заводит/обновляет организацию,
//!   филиалы и пользователей по устойчивым id сайта. Права сотрудника в Атласе следуют правам дев-панели.
//! - `GET /api/site/sales?from&to` — движения денег за рабочие дни [from, to) для сверки с кассой:
//!   продажи (смешанная оплата — двумя строками), возвраты деньгами (отрицательная сумма) и оплаты
//!   комплексных продаж (предоплаты считаются в день, когда пришли деньги). Суммы — целые копейки.
//!
//! Рабочий день: дата открытия смены филиала, в которую попала операция (как у кассового отчёта);
//! без смены — дата по поясу филиала со сдвигом 06:00 (ночные операции до 6 утра — предыдущий день).
use crate::{
    auth::hash_password,
    error::{AppError, AppResult},
    AppState,
};
use axum::{
    extract::{Query, State},
    http::HeaderMap,
    routing::{get, post},
    Json, Router,
};
use chrono::{Duration, NaiveDate};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
// Динамический SQL здесь собирается только из констант (BUSINESS_DATE) — без пользовательского ввода.
use sqlx::Row;
use std::collections::HashMap;
use uuid::Uuid;

pub const VIEWER_KEY: &str = "site_viewer";
const VIEWER_PERMS: &[&str] = &[
    "sale.history",
    "product.view",
    "stock.view",
    "client.view",
    "client.card",
    "shift.view",
    "reports.view",
    "reports.home_stats",
    "settings.other_stores",
    "settings.switch_store",
];

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/sync", post(sync))
        .route("/sales", get(sales))
        .route("/clients", get(clients))
        .route("/cashbox", get(site_cashbox).post(site_cashbox_add))
}

/// Движение кассы, присланное сайтом (инкассация в дев-панели).
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SiteCashboxDto {
    /// id отчёта инкассации на сайте — защита от дублей при повторной отправке
    external_id: String,
    /// филиал сайта (external_id магазина)
    branch_id: String,
    /// рабочий день отчёта, YYYY-MM-DD
    date: String,
    /// со знаком: изъятие < 0, внесение > 0 (как в cashbox_transactions)
    amount: f64,
    comment: Option<String>,
    /// сотрудник сайта (staffId) — по нему находим пользователя Атласа
    staff_id: Option<String>,
}

/// Сайт присылает запись инкассации — она становится движением кассы смены.
/// Операция привязывается к существующей смене этого филиала и рабочего дня.
async fn site_cashbox_add(State(state): State<AppState>, headers: HeaderMap, Json(dto): Json<SiteCashboxDto>) -> AppResult<Json<Value>> {
    super::auth::check_site_secret(&headers)?;
    let external_id = dto.external_id.trim();
    if external_id.is_empty() || external_id.len() > 80 {
        return Err(AppError::BadRequest("Нет id отчёта инкассации".into()));
    }
    if !dto.amount.is_finite() || dto.amount == 0.0 {
        return Err(AppError::BadRequest("Сумма должна быть не равна нулю".into()));
    }
    let day = NaiveDate::parse_from_str(dto.date.trim(), "%Y-%m-%d").map_err(|_| AppError::BadRequest("Дата — YYYY-MM-DD".into()))?;

    let (org_id,): (Uuid,) = sqlx::query_as("SELECT id FROM organizations WHERE external_id='wow-my-eyes'")
        .fetch_optional(&state.pool)
        .await?
        .ok_or_else(|| AppError::NotFound("Организация сайта ещё не заведена".into()))?;

    let store: Option<(Uuid, String)> = sqlx::query_as(
        "SELECT id, timezone FROM stores WHERE organization_id=$1 AND external_id=$2",
    )
    .bind(org_id)
    .bind(dto.branch_id.trim())
    .fetch_optional(&state.pool)
    .await?;
    let Some((store_id, tz)) = store else {
        return Err(AppError::NotFound("Такой филиал в CRM не заведён".into()));
    };

    // Смена этого филиала на рабочий день: по дате открытия в поясе филиала.
    let shift: Option<(Uuid, Uuid)> = sqlx::query_as(
        "SELECT id, opened_by FROM shifts WHERE organization_id=$1 AND store_id=$2 \
           AND (opened_at AT TIME ZONE $3)::date = $4::date ORDER BY opened_at DESC LIMIT 1",
    )
    .bind(org_id).bind(store_id).bind(&tz).bind(day)
    .fetch_optional(&state.pool)
    .await?;
    let Some((shift_id, opener_id)) = shift else {
        return Err(AppError::BadRequest("На эту дату в филиале нет смены — сначала откройте смену".into()));
    };

    // Сотрудник: у него на сайте есть staffId, в Атласе — staff_external_id.
    let user_id: Option<(Uuid,)> = match dto.staff_id.as_deref().map(str::trim).filter(|v| !v.is_empty()) {
        Some(sid) => sqlx::query_as("SELECT id FROM users WHERE organization_id=$1 AND staff_external_id=$2 LIMIT 1")
            .bind(org_id).bind(sid).fetch_optional(&state.pool).await?,
        None => None,
    };
    let user_id = match user_id {
        Some((id,)) => id,
        None => opener_id,
    };

    let mut tx = state.pool.begin().await?;
    let inserted: Option<(Uuid,)> = sqlx::query_as(
        "INSERT INTO cashbox_transactions (id, organization_id, shift_id, user_id, amount, comment, external_id, source) \
         VALUES ($1,$2,$3,$4,$5::float8,$6,$7,'site') \
         ON CONFLICT (organization_id, external_id) WHERE external_id IS NOT NULL DO NOTHING RETURNING id",
    )
    .bind(Uuid::new_v4()).bind(org_id).bind(shift_id).bind(user_id)
    .bind(dto.amount)
    .bind(dto.comment.clone().unwrap_or_default())
    .bind(external_id)
    .fetch_optional(&mut *tx)
    .await?;

    // Если отчёт внесли после закрытия смены, обновить сохранённые ожидание и
    // расхождение. Для открытой смены refresh_closed_shift ничего не меняет.
    super::shifts::refresh_closed_shift(&mut tx, org_id, shift_id).await?;
    tx.commit().await?;

    Ok(Json(json!({
        "ok": true,
        "shiftId": shift_id,
        "duplicate": inserted.is_none(),
    })))
}

async fn site_cashbox(State(state): State<AppState>, headers: HeaderMap, Query(q): Query<SalesQuery>) -> AppResult<Json<Value>> {
    super::auth::check_site_secret(&headers)?;
    let from = NaiveDate::parse_from_str(&q.from, "%Y-%m-%d").map_err(|_| AppError::BadRequest("Неверная дата начала".into()))?;
    let to = NaiveDate::parse_from_str(&q.to, "%Y-%m-%d").map_err(|_| AppError::BadRequest("Неверная дата конца".into()))?;
    if to <= from || (to - from).num_days() > 366 { return Err(AppError::BadRequest("Неверный период".into())); }
    let rows = sqlx::query(
        "SELECT t.id, s.id AS shift_id, st.external_id AS branch_id, (s.opened_at AT TIME ZONE 'Europe/Moscow')::date AS date, \
                t.amount::float8 AS amount, t.comment, t.created_at \
         FROM cashbox_transactions t JOIN shifts s ON s.id=t.shift_id \
         JOIN stores st ON st.id=s.store_id JOIN organizations o ON o.id=t.organization_id \
         WHERE o.external_id='wow-my-eyes' AND (s.opened_at AT TIME ZONE 'Europe/Moscow')::date >= $1 \
           AND (s.opened_at AT TIME ZONE 'Europe/Moscow')::date < $2 ORDER BY t.created_at DESC"
    ).bind(from).bind(to).fetch_all(&state.pool).await?;
    Ok(Json(json!({"rows": rows.iter().map(|r| json!({
        "id": r.get::<Uuid,_>("id"), "shiftId": r.get::<Uuid,_>("shift_id"),
        "branchId": r.get::<String,_>("branch_id"), "date": r.get::<NaiveDate,_>("date"),
        "amount": (r.get::<f64,_>("amount") * 100.0).round() as i64,
        "comment": r.get::<String,_>("comment"), "createdAt": r.get::<chrono::DateTime<chrono::Utc>,_>("created_at")
    })).collect::<Vec<_>>() })))
}

#[derive(Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
struct ClientContact {
    phone: String,
    name: String,
    email: String,
    first_at: chrono::DateTime<chrono::Utc>,
    last_at: chrono::DateTime<chrono::Utc>,
}

async fn clients(State(state): State<AppState>, headers: HeaderMap) -> AppResult<Json<Vec<ClientContact>>> {
    super::auth::check_site_secret(&headers)?;
    let contacts = sqlx::query_as::<_, ClientContact>(
        "SELECT phone, MAX(name) AS name, MAX(email) AS email, MIN(at) AS first_at, MAX(at) AS last_at FROM ( \
           SELECT c.phone, c.name, ''::text AS email, s.sale_date AS at \
           FROM clients c JOIN organizations o ON o.id=c.organization_id \
           JOIN sales s ON s.client_id=c.id AND s.organization_id=c.organization_id AND s.deleted_at IS NULL \
           WHERE o.external_id='wow-my-eyes' AND c.deleted_at IS NULL AND c.phone<>'' \
           UNION ALL \
           SELECT co.client_phone AS phone, co.client_name AS name, co.client_email AS email, co.created_at AS at \
           FROM client_orders co JOIN organizations o ON o.id=co.organization_id \
           WHERE o.external_id='wow-my-eyes' AND co.deleted_at IS NULL AND co.client_phone<>'' \
         ) contacts GROUP BY phone ORDER BY MAX(at) DESC",
    ).fetch_all(&state.pool).await?;
    Ok(Json(contacts))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoreIn {
    pub external_id: String,
    pub name: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserIn {
    pub external_id: String,
    #[serde(default)]
    pub staff_id: Option<String>,
    pub login: String,
    pub name: String,
    /// admin — всё; seller — вести продажи; viewer — только смотреть.
    pub role: String,
    /// None — все филиалы из `stores`.
    #[serde(default)]
    pub store_external_ids: Option<Vec<String>>,
    pub active: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncDto {
    pub organization_external_id: String,
    pub organization_name: String,
    pub stores: Vec<StoreIn>,
    pub users: Vec<UserIn>,
}

fn site_email(login: &str) -> String {
    format!("{}@staff.site", login.trim().to_lowercase())
}

/// Кассовые смены сайта заканчиваются в полночь по часовому поясу филиала.
/// (граница рабочего дня та же, что у кассы и у выгрузки `/api/site/sales`). Потом можно поменять в настройках филиала.
const SITE_AUTO_CLOSE: &str = "00:00";

/// Способы оплаты, нужные для сверки с кассой (помимо «Наличные», «QR», «Терминал» по умолчанию).
const SITE_METHODS: &[(&str, &str)] = &[("Перевод", "transfer"), ("Сертификат", "cert")];

async fn sync(State(state): State<AppState>, headers: HeaderMap, Json(dto): Json<SyncDto>) -> AppResult<Json<Value>> {
    super::auth::check_site_secret(&headers)?;
    let ext = dto.organization_external_id.trim();
    if ext.is_empty() || dto.stores.is_empty() {
        return Err(AppError::BadRequest("Нужны организация и хотя бы один филиал".into()));
    }
    let mut tx = state.pool.begin().await?;
    let found: Option<(Uuid,)> = sqlx::query_as("SELECT id FROM organizations WHERE external_id=$1").bind(ext).fetch_optional(&mut *tx).await?;
    let mut created = false;
    let org_id = match found {
        Some((id,)) => id,
        None => {
            created = true;
            let first = &dto.stores[0];
            let (org_id, store_id, _) = super::auth::create_organization(&mut tx, dto.organization_name.trim(), first.name.trim()).await?;
            sqlx::query("UPDATE organizations SET external_id=$2, modules = modules || '{\"shifts\": true}'::jsonb WHERE id=$1")
                .bind(org_id)
                .bind(ext)
                .execute(&mut *tx)
                .await?;
            sqlx::query("UPDATE stores SET external_id=$2, auto_close_time=$3 WHERE id=$1").bind(store_id).bind(first.external_id.trim()).bind(SITE_AUTO_CLOSE).execute(&mut *tx).await?;
            for (i, (name, code)) in SITE_METHODS.iter().enumerate() {
                sqlx::query("INSERT INTO list_items (id, organization_id, kind, name, params, sort_order) VALUES ($1,$2,'payment_methods',$3,$4,$5)")
                    .bind(Uuid::new_v4())
                    .bind(org_id)
                    .bind(*name)
                    .bind(json!({ "kind": code, "siteCode": code }))
                    .bind(10 + i as i32)
                    .execute(&mut *tx)
                    .await?;
            }
            org_id
        }
    };

    // Филиалы
    let mut store_map: HashMap<String, Uuid> = HashMap::new();
    for st in &dto.stores {
        let e = st.external_id.trim();
        let row: Option<(Uuid,)> = sqlx::query_as("SELECT id FROM stores WHERE organization_id=$1 AND external_id=$2")
            .bind(org_id)
            .bind(e)
            .fetch_optional(&mut *tx)
            .await?;
        let id = match row {
            Some((id,)) => {
                sqlx::query("UPDATE stores SET name=$2, deleted_at=NULL, auto_close_time=$3 WHERE id=$1")
                    .bind(id).bind(st.name.trim()).bind(SITE_AUTO_CLOSE).execute(&mut *tx).await?;
                id
            }
            None => {
                let id = Uuid::new_v4();
                sqlx::query("INSERT INTO stores (id, organization_id, name, external_id, auto_close_time) VALUES ($1,$2,$3,$4,$5)")
                    .bind(id)
                    .bind(org_id)
                    .bind(st.name.trim())
                    .bind(e)
                    .bind(SITE_AUTO_CLOSE)
                    .execute(&mut *tx)
                    .await?;
                id
            }
        };
        store_map.insert(e.to_string(), id);
    }

    // Роли: «Админ», «Продавец» — по умолчанию; «Просмотр (сайт)» — заводим, если нет.
    let roles: Vec<(Uuid, String)> = sqlx::query_as("SELECT id, key FROM roles WHERE organization_id=$1").bind(org_id).fetch_all(&mut *tx).await?;
    let mut role_of: HashMap<String, Uuid> = roles.into_iter().map(|(id, k)| (k, id)).collect();
    if !role_of.contains_key(VIEWER_KEY) {
        let id = Uuid::new_v4();
        let perms: Vec<String> = VIEWER_PERMS.iter().map(|s| s.to_string()).collect();
        sqlx::query("INSERT INTO roles (id, organization_id, key, name, permissions) VALUES ($1,$2,$3,'Просмотр (сайт)',$4)")
            .bind(id)
            .bind(org_id)
            .bind(VIEWER_KEY)
            .bind(&perms)
            .execute(&mut *tx)
            .await?;
        role_of.insert(VIEWER_KEY.into(), id);
    }
    let role_id = |r: &str| -> AppResult<Uuid> {
        let key = match r {
            "admin" => atlas_domain::roles::ADMIN_KEY,
            "seller" => atlas_domain::roles::CASHIER_KEY,
            "viewer" => VIEWER_KEY,
            _ => return Err(AppError::BadRequest(format!("Неизвестная роль «{r}»"))),
        };
        role_of.get(key).copied().ok_or_else(|| AppError::BadRequest(format!("В Атласе нет роли «{key}»")))
    };
    let all_stores: Vec<Uuid> = dto.stores.iter().filter_map(|s| store_map.get(s.external_id.trim()).copied()).collect();

    // Сотрудники
    let mut seen: Vec<String> = Vec::new();
    let (mut added, mut updated) = (0, 0);
    for u in &dto.users {
        let e = u.external_id.trim().to_string();
        if e.is_empty() || u.login.trim().is_empty() {
            return Err(AppError::BadRequest("У сотрудника нет id или логина".into()));
        }
        let rid = role_id(&u.role)?;
        let stores: Vec<Uuid> = match &u.store_external_ids {
            None => all_stores.clone(),
            Some(ids) => ids.iter().filter_map(|i| store_map.get(i.trim()).copied()).collect(),
        };
        let active = u.active && !stores.is_empty();
        let staff = u.staff_id.as_deref().map(str::trim).filter(|s| !s.is_empty());
        let row: Option<(Uuid,)> = sqlx::query_as("SELECT id FROM users WHERE organization_id=$1 AND external_id=$2")
            .bind(org_id)
            .bind(&e)
            .fetch_optional(&mut *tx)
            .await?;
        match row {
            Some((id,)) => {
                sqlx::query("UPDATE users SET email=$2, name=$3, role_id=$4, store_ids=$5, active=$6, staff_external_id=$7 WHERE id=$1")
                    .bind(id)
                    .bind(site_email(&u.login))
                    .bind(u.name.trim())
                    .bind(rid)
                    .bind(&stores)
                    .bind(active)
                    .bind(staff)
                    .execute(&mut *tx)
                    .await?;
                updated += 1;
            }
            None => {
                // Пароля нет: сотрудник входит только с сайта. Хеш случайной строки никто не знает.
                let hash = hash_password(&format!("{}{}", Uuid::new_v4(), Uuid::new_v4()))?;
                sqlx::query(
                    "INSERT INTO users (id, organization_id, email, name, password_hash, role_id, store_ids, active, external_id, staff_external_id) \
                     VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)",
                )
                .bind(Uuid::new_v4())
                .bind(org_id)
                .bind(site_email(&u.login))
                .bind(u.name.trim())
                .bind(hash)
                .bind(rid)
                .bind(&stores)
                .bind(active)
                .bind(&e)
                .bind(staff)
                .execute(&mut *tx)
                .await?;
                added += 1;
            }
        }
        seen.push(e);
    }
    // Кого сайт больше не прислал — выключаем (история продаж остаётся).
    let off = sqlx::query("UPDATE users SET active=false WHERE organization_id=$1 AND external_id IS NOT NULL AND NOT (external_id = ANY($2)) AND active")
        .bind(org_id)
        .bind(&seen)
        .execute(&mut *tx)
        .await?
        .rows_affected();
    tx.commit().await?;
    Ok(Json(json!({ "organizationId": org_id, "created": created, "stores": store_map.len(), "usersAdded": added, "usersUpdated": updated, "usersDeactivated": off })))
}

#[derive(Deserialize)]
pub struct SalesQuery {
    pub from: String,
    pub to: String,
}

/// Строка для сверки — тот же смысл полей, что у заказа CRM на сайте.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SiteMoneyRow {
    pub id: String,
    /// sale — продажа; return — возврат деньгами (сумма < 0); payment — оплата комплексной продажи (в т. ч. предоплата).
    pub kind: &'static str,
    pub sale_id: Uuid,
    pub branch_id: String,
    pub date: NaiveDate,
    pub time: String,
    pub staff_id: String,
    pub user_external_id: String,
    pub user_name: String,
    pub title: String,
    pub comment: String,
    pub client_phone: String,
    pub amount: i64,
    pub pay_type: String,
    pub category: String,
    pub shift_id: Option<Uuid>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// Способ оплаты → код сайта: cash / card / transfer / cert.
pub fn pay_code(name: &str, params: &Value) -> &'static str {
    let by = |v: &str| match v {
        "cash" => Some("cash"),
        "terminal" | "qr" | "card" => Some("card"),
        "transfer" => Some("transfer"),
        "cert" | "certificate" => Some("cert"),
        _ => None,
    };
    if let Some(c) = params.get("siteCode").and_then(Value::as_str).and_then(by) {
        return c;
    }
    if let Some(c) = params.get("kind").and_then(Value::as_str).and_then(by) {
        return c;
    }
    wallet_code(name, false)
}

/// Денежный счёт (оплаты комплексных продаж) → код сайта.
pub fn wallet_code(name: &str, cash_register: bool) -> &'static str {
    let n = name.to_lowercase();
    if cash_register || n.contains("налич") || n.starts_with("нал") {
        "cash"
    } else if n.contains("серт") {
        "cert"
    } else if n.contains("терминал") || n.contains("эквайр") || n.contains("qr") || n.contains("безнал") {
        "card"
    } else {
        "transfer"
    }
}

fn category_of(cats: &str) -> &'static str {
    let c = cats.to_lowercase();
    if c.contains("аксесс") {
        "accessory"
    } else if c.contains("картин") {
        "painting"
    } else {
        "main"
    }
}

/// Рабочий день операции в момент `ts` филиала `store`: дата открытия смены, накрывающей момент, иначе 06:00-граница.
const BUSINESS_DATE: &str = "COALESCE(\
    (SELECT (sh.opened_at AT TIME ZONE st.timezone)::date FROM shifts sh \
      WHERE sh.store_id = st.id AND sh.opened_at <= {ts} AND (sh.closed_at IS NULL OR sh.closed_at >= {ts}) \
      ORDER BY sh.opened_at DESC LIMIT 1), \
    (({ts} AT TIME ZONE st.timezone) - interval '6 hours')::date)";

fn bdate(ts: &str) -> String {
    BUSINESS_DATE.replace("{ts}", ts)
}

async fn sales(State(state): State<AppState>, headers: HeaderMap, Query(q): Query<SalesQuery>) -> AppResult<Json<Value>> {
    super::auth::check_site_secret(&headers)?;
    let parse = |s: &str| NaiveDate::parse_from_str(s, "%Y-%m-%d").map_err(|_| AppError::BadRequest("Даты — YYYY-MM-DD".into()));
    let (from, to) = (parse(&q.from)?, parse(&q.to)?);
    if to <= from || (to - from).num_days() > 400 {
        return Err(AppError::BadRequest("Период [from, to) до 400 дней".into()));
    }
    let org: Option<(Uuid,)> = sqlx::query_as("SELECT id FROM organizations WHERE external_id IS NOT NULL ORDER BY created_at LIMIT 1")
        .fetch_optional(&state.pool)
        .await?;
    let Some((org_id,)) = org else {
        return Ok(Json(json!({ "rows": [], "note": "организация сайта ещё не заведена" })));
    };
    // Окно по времени с запасом в сутки с каждой стороны; точный фильтр — по рабочему дню.
    let lo = (from - Duration::days(1)).and_hms_opt(0, 0, 0).unwrap().and_utc();
    let hi = (to + Duration::days(2)).and_hms_opt(0, 0, 0).unwrap().and_utc();

    let methods: Vec<(Uuid, String, Value)> =
        sqlx::query_as("SELECT id, name, params FROM list_items WHERE organization_id=$1 AND kind='payment_methods'").bind(org_id).fetch_all(&state.pool).await?;
    let code: HashMap<Uuid, &'static str> = methods.iter().map(|(id, n, p)| (*id, pay_code(n, p))).collect();
    let code_of = |id: Option<Uuid>| id.and_then(|i| code.get(&i).copied()).unwrap_or("cash").to_string();

    let mut rows: Vec<SiteMoneyRow> = Vec::new();

    // Продажи
    let sql = format!(
        "SELECT s.id, st.external_id, COALESCE(u.staff_external_id,''), COALESCE(u.external_id,''), u.name, \
           CASE WHEN sh.opened_at IS NOT NULL THEN (sh.opened_at AT TIME ZONE st.timezone)::date ELSE {bd} END AS bdate, \
           to_char(s.sale_date AT TIME ZONE st.timezone, 'HH24:MI'), round(s.total*100)::int8, \
           s.payment_method_id, s.second_payment_method_id, round(COALESCE(s.first_payment_value,0)*100)::int8, round(COALESCE(s.second_payment_value,0)*100)::int8, \
           s.comment, COALESCE(c.phone,''), s.created_at, s.shift_id, \
           COALESCE((SELECT string_agg(si.name, ', ' ORDER BY si.name) FROM sale_items si WHERE si.sale_id=s.id),''), \
           COALESCE((SELECT string_agg(DISTINCT pc.name, '|') FROM sale_items si JOIN products p ON p.id=si.product_id \
                     JOIN product_categories pc ON pc.id=p.category_id WHERE si.sale_id=s.id),'') \
         FROM sales s JOIN stores st ON st.id=s.store_id JOIN users u ON u.id=s.user_id \
         LEFT JOIN clients c ON c.id=s.client_id LEFT JOIN shifts sh ON sh.id=s.shift_id \
         WHERE s.organization_id=$1 AND s.deleted_at IS NULL AND st.external_id IS NOT NULL AND s.sale_date >= $2 AND s.sale_date < $3",
        bd = bdate("s.sale_date")
    );
    for r in sqlx::query(sqlx::AssertSqlSafe(sql)).bind(org_id).bind(lo).bind(hi).fetch_all(&state.pool).await? {
        let date: NaiveDate = r.try_get(5)?;
        if date < from || date >= to {
            continue;
        }
        let sale_id: Uuid = r.try_get(0)?;
        let total: i64 = r.try_get(7)?;
        let (m1, m2): (Option<Uuid>, Option<Uuid>) = (r.try_get(8)?, r.try_get(9)?);
        let (v1, v2): (i64, i64) = (r.try_get(10)?, r.try_get(11)?);
        let base = |id: String, amount: i64, pay: String| -> AppResult<SiteMoneyRow> {
            Ok(SiteMoneyRow {
                id,
                kind: "sale",
                sale_id,
                branch_id: r.try_get(1)?,
                date,
                time: r.try_get(6)?,
                staff_id: r.try_get(2)?,
                user_external_id: r.try_get(3)?,
                user_name: r.try_get(4)?,
                title: r.try_get(16)?,
                comment: r.try_get(12)?,
                client_phone: r.try_get(13)?,
                amount,
                pay_type: pay,
                category: category_of(&r.try_get::<String, _>(17)?).into(),
                shift_id: r.try_get(15)?,
                created_at: r.try_get(14)?,
            })
        };
        if m2.is_some() && v2 > 0 {
            let first = if v1 > 0 { v1 } else { total - v2 };
            rows.push(base(sale_id.to_string(), first, code_of(m1))?);
            rows.push(base(format!("{sale_id}:2"), total - first, code_of(m2))?);
        } else {
            rows.push(base(sale_id.to_string(), total, code_of(m1))?);
        }
    }

    // Возвраты деньгами
    let sql = format!(
        "SELECT r.id, s.id, st.external_id, COALESCE(u.staff_external_id,''), COALESCE(u.external_id,''), u.name, {bd}, \
           to_char(r.created_at AT TIME ZONE st.timezone, 'HH24:MI'), round(r.amount*100)::int8, s.payment_method_id, r.comment, \
           COALESCE(c.phone,''), r.created_at \
         FROM sale_returns r JOIN sales s ON s.id=r.sale_id JOIN stores st ON st.id=s.store_id JOIN users u ON u.id=r.user_id \
         LEFT JOIN clients c ON c.id=s.client_id \
         WHERE r.organization_id=$1 AND r.return_type='money' AND s.deleted_at IS NULL AND st.external_id IS NOT NULL \
           AND r.created_at >= $2 AND r.created_at < $3",
        bd = bdate("r.created_at")
    );
    for r in sqlx::query(sqlx::AssertSqlSafe(sql)).bind(org_id).bind(lo).bind(hi).fetch_all(&state.pool).await? {
        let date: NaiveDate = r.try_get(6)?;
        if date < from || date >= to {
            continue;
        }
        let id: Uuid = r.try_get(0)?;
        let amount: i64 = r.try_get(8)?;
        rows.push(SiteMoneyRow {
            id: format!("return:{id}"),
            kind: "return",
            sale_id: r.try_get(1)?,
            branch_id: r.try_get(2)?,
            date,
            time: r.try_get(7)?,
            staff_id: r.try_get(3)?,
            user_external_id: r.try_get(4)?,
            user_name: r.try_get(5)?,
            title: "Возврат".into(),
            comment: r.try_get(10)?,
            client_phone: r.try_get(11)?,
            amount: -amount,
            pay_type: code_of(r.try_get(9)?),
            category: "main".into(),
            shift_id: None,
            created_at: r.try_get(12)?,
        });
    }

    // Оплаты комплексных продаж (предоплаты и доплаты) — в день поступления денег.
    let sql = format!(
        "SELECT p.id, cs.id, st.external_id, COALESCE(u.staff_external_id,''), COALESCE(u.external_id,''), u.name, {bd}, \
           to_char(p.paid_at AT TIME ZONE st.timezone, 'HH24:MI'), round(p.amount*100)::int8, COALESCE(w.name,''), COALESCE(w.is_cash_register,false), \
           cs.name, p.comment, COALESCE(c.phone,''), p.paid_at \
         FROM complex_sale_payments p JOIN complex_sales cs ON cs.id=p.sale_id JOIN stores st ON st.id=cs.store_id \
         JOIN users u ON u.id=COALESCE(p.user_id, cs.user_id) LEFT JOIN wallets w ON w.id=p.wallet_id LEFT JOIN clients c ON c.id=cs.client_id \
         WHERE cs.organization_id=$1 AND cs.deleted_at IS NULL AND st.external_id IS NOT NULL AND p.paid_at >= $2 AND p.paid_at < $3",
        bd = bdate("p.paid_at")
    );
    for r in sqlx::query(sqlx::AssertSqlSafe(sql)).bind(org_id).bind(lo).bind(hi).fetch_all(&state.pool).await? {
        let date: NaiveDate = r.try_get(6)?;
        if date < from || date >= to {
            continue;
        }
        let id: Uuid = r.try_get(0)?;
        let wallet: String = r.try_get(9)?;
        let name: String = r.try_get(11)?;
        rows.push(SiteMoneyRow {
            id: format!("payment:{id}"),
            kind: "payment",
            sale_id: r.try_get(1)?,
            branch_id: r.try_get(2)?,
            date,
            time: r.try_get(7)?,
            staff_id: r.try_get(3)?,
            user_external_id: r.try_get(4)?,
            user_name: r.try_get(5)?,
            title: format!("{name} — оплата заказа"),
            comment: r.try_get(12)?,
            client_phone: r.try_get(13)?,
            amount: r.try_get(8)?,
            pay_type: wallet_code(&wallet, r.try_get(10)?).into(),
            category: "main".into(),
            shift_id: None,
            created_at: r.try_get(14)?,
        });
    }
    rows.sort_by(|a, b| (a.date, &a.time, &a.id).cmp(&(b.date, &b.time, &b.id)));
    Ok(Json(json!({ "rows": rows, "moneyUnit": "kopecks", "from": from, "to": to })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn payment_codes() {
        assert_eq!(pay_code("Наличные", &json!({"kind": "cash"})), "cash");
        assert_eq!(pay_code("QR", &json!({"kind": "qr"})), "card");
        assert_eq!(pay_code("Терминал", &json!({"kind": "terminal"})), "card");
        assert_eq!(pay_code("Перевод", &json!({"kind": "transfer", "siteCode": "transfer"})), "transfer");
        assert_eq!(pay_code("Сертификат", &json!({})), "cert");
        assert_eq!(pay_code("Перевод на карту", &json!({})), "transfer");
        assert_eq!(wallet_code("Наличные с кассы 0 этаж", true), "cash");
        assert_eq!(category_of("Аксессуары|Кулоны"), "accessory");
    }
}
