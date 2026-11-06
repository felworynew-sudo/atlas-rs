//! Atlas API v1 (/api/v1/*): доступ по API-токену организации — чтение данных, отчёты и черновики продаж/приёмок
use crate::{
    auth::AuthContext,
    error::{AppError, AppResult},
    routes::{
        api_tokens::{audit_log, DraftView, SELECT_DRAFTS},
        documents::{fetch_supplies, SupplyQuery},
        products::fetch_views,
        sales::{fetch_sales, ListQuery},
    },
    AppState,
};
use atlas_domain::sale::round2;
use axum::{
    extract::{FromRequestParts, Path, Query, State},
    http::request::Parts,
    routing::{get, post},
    Json, Router,
};
use chrono::{DateTime, Duration, NaiveDate, Utc};
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/whoami", get(whoami))
        .route("/stores", get(stores))
        .route("/products", get(products))
        .route("/products/{id}", get(product_one))
        .route("/sales", get(sales))
        .route("/sales/{id}", get(sale_one))
        .route("/acceptances", get(acceptances))
        .route("/reports/revenue-by-day", get(revenue_by_day))
        .route("/reports/sales-summary", get(sales_summary))
        .route("/reports/top-products", get(top_products))
        .route("/reports/stock-balances", get(stock_balances))
        .route("/reports/stock-balances/{id}", get(stock_balance_one))
        .route("/sales/drafts", post(sale_draft))
        .route("/acceptances/drafts", post(acceptance_draft))
        .route("/drafts", get(drafts))
        .route("/drafts/{id}", get(draft_one))
}

/// Вызывающий по API-токену: действует от имени создавшего токен администратора и только в своей организации.
pub struct ApiAuth {
    pub token_id: Uuid,
    pub token_name: String,
    pub scopes: Vec<String>,
    pub expires_at: DateTime<Utc>,
    pub ctx: AuthContext,
}

impl FromRequestParts<AppState> for ApiAuth {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Self::Rejection> {
        let secret = parts
            .headers
            .get(axum::http::header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .filter(|t| t.starts_with("atl_"))
            .ok_or_else(|| AppError::Unauthorized("Нужен API-токен (Bearer atl_…)".into()))?;
        let bad = || AppError::Unauthorized("Токен недействителен, отозван или истёк".into());
        let tok: Option<(Uuid, String, Vec<String>, Uuid, DateTime<Utc>)> = sqlx::query_as(
            "SELECT id, name, scopes, created_by, expires_at FROM api_tokens \
             WHERE token_hash = encode(sha256(convert_to($1,'UTF8')),'hex') AND revoked_at IS NULL AND expires_at > now()",
        )
        .bind(secret)
        .fetch_optional(&state.pool)
        .await?;
        let (token_id, token_name, scopes, creator, expires_at) = tok.ok_or_else(bad)?;
        let row: Option<(Uuid, String, Vec<String>, bool, Value)> = sqlx::query_as(
            "SELECT u.organization_id, r.key, r.permissions, u.active, o.modules FROM users u \
             JOIN roles r ON r.id = u.role_id JOIN organizations o ON o.id = u.organization_id WHERE u.id = $1",
        )
        .bind(creator)
        .fetch_optional(&state.pool)
        .await?;
        let Some((org_id, role_key, permissions, true, modules)) = row else { return Err(bad()) };
        let _ = sqlx::query("UPDATE api_tokens SET last_used_at = now() WHERE id = $1").bind(token_id).execute(&state.pool).await;
        Ok(ApiAuth { token_id, token_name, scopes, expires_at, ctx: AuthContext { user_id: creator, org_id, role_key, permissions, modules } })
    }
}

impl ApiAuth {
    /// `read` даёт все права чтения; остальные права проверяются точно.
    fn allows(&self, scope: &str) -> bool {
        self.scopes.iter().any(|s| s == scope || (s == "read" && scope.starts_with("read")))
    }

    /// Проверка права с записью в журнал: «Агент «имя» <действие>» либо «… — отказано».
    async fn check(&self, s: &AppState, scope: &[&str], action: &str, verb: &str) -> AppResult<()> {
        let ok = scope.iter().any(|sc| self.allows(sc));
        let text = if ok { format!("Агент «{}» {verb}", self.token_name) } else { format!("Агент «{}» {verb} — отказано", self.token_name) };
        audit_log(&s.pool, self.ctx.org_id, Some(self.token_id), Some(self.ctx.user_id), action, ok, &text).await;
        if ok {
            Ok(())
        } else {
            Err(AppError::Forbidden)
        }
    }
}

async fn whoami(State(s): State<AppState>, a: ApiAuth) -> AppResult<Json<Value>> {
    let (org,): (String,) = sqlx::query_as("SELECT name FROM organizations WHERE id=$1").bind(a.ctx.org_id).fetch_one(&s.pool).await?;
    let (user,): (String,) = sqlx::query_as("SELECT name FROM users WHERE id=$1").bind(a.ctx.user_id).fetch_one(&s.pool).await?;
    audit_log(&s.pool, a.ctx.org_id, Some(a.token_id), Some(a.ctx.user_id), "whoami", true, &format!("Агент «{}» запросил сведения о токене", a.token_name)).await;
    Ok(Json(json!({
        "organization": { "id": a.ctx.org_id, "name": org },
        "user": { "id": a.ctx.user_id, "name": user },
        "token": { "id": a.token_id, "name": a.token_name, "scopes": a.scopes, "expiresAt": a.expires_at },
    })))
}

async fn stores(State(s): State<AppState>, a: ApiAuth) -> AppResult<Json<Vec<Value>>> {
    audit_log(&s.pool, a.ctx.org_id, Some(a.token_id), Some(a.ctx.user_id), "stores", true, &format!("Агент «{}» запросил филиалы", a.token_name)).await;
    let rows: Vec<(Uuid, String, String)> =
        sqlx::query_as("SELECT id, name, timezone FROM stores WHERE organization_id=$1 AND deleted_at IS NULL ORDER BY created_at").bind(a.ctx.org_id).fetch_all(&s.pool).await?;
    Ok(Json(rows.into_iter().map(|(id, name, tz)| json!({"id": id, "name": name, "timezone": tz})).collect()))
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ListQ {
    pub search: Option<String>,
    pub store_id: Option<Uuid>,
    pub from: Option<DateTime<Utc>>,
    pub to: Option<DateTime<Utc>>,
    pub limit: Option<usize>,
}

fn limit(q: &ListQ) -> usize {
    q.limit.unwrap_or(100).clamp(1, 500)
}

async fn products(State(s): State<AppState>, a: ApiAuth, Query(q): Query<ListQ>) -> AppResult<Json<Vec<Value>>> {
    a.check(&s, &["read:products"], "products.list", "запросил товары").await?;
    let rows = fetch_views(&s, &a.ctx, q.search.clone(), None, None, q.store_id, true).await?;
    Ok(Json(rows.into_iter().take(limit(&q)).map(|p| serde_json::to_value(p).unwrap_or(Value::Null)).collect()))
}

async fn product_one(State(s): State<AppState>, a: ApiAuth, Path(id): Path<Uuid>) -> AppResult<Json<Value>> {
    a.check(&s, &["read:products"], "products.get", "запросил товар").await?;
    let p = fetch_views(&s, &a.ctx, None, None, Some(id), None, true)
        .await?
        .into_iter()
        .next()
        .ok_or_else(|| AppError::NotFound("Товар не найден".into()))?;
    Ok(Json(serde_json::to_value(p).map_err(|e| AppError::Internal(e.to_string()))?))
}

async fn sales(State(s): State<AppState>, a: ApiAuth, Query(q): Query<ListQ>) -> AppResult<Json<Vec<Value>>> {
    a.check(&s, &["read:sales"], "sales.list", "запросил продажи").await?;
    let rows = fetch_sales(&s.pool, a.ctx.org_id, None, &ListQuery { store_id: q.store_id, from: q.from, to: q.to, ..Default::default() }).await?;
    Ok(Json(rows.into_iter().take(limit(&q)).map(|x| serde_json::to_value(x).unwrap_or(Value::Null)).collect()))
}

async fn sale_one(State(s): State<AppState>, a: ApiAuth, Path(id): Path<Uuid>) -> AppResult<Json<Value>> {
    a.check(&s, &["read:sales"], "sales.get", "запросил продажу").await?;
    let sale = fetch_sales(&s.pool, a.ctx.org_id, Some(id), &ListQuery::default())
        .await?
        .into_iter()
        .next()
        .ok_or_else(|| AppError::NotFound("Продажа не найдена".into()))?;
    Ok(Json(serde_json::to_value(sale).map_err(|e| AppError::Internal(e.to_string()))?))
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct DateQ {
    pub store_id: Option<Uuid>,
    pub from: Option<NaiveDate>,
    pub to: Option<NaiveDate>,
    pub limit: Option<i64>,
}

async fn acceptances(State(s): State<AppState>, a: ApiAuth, Query(q): Query<DateQ>) -> AppResult<Json<Vec<Value>>> {
    // приёмки входят только в «чтение — всё»
    a.check(&s, &["read:all"], "acceptances.list", "запросил приёмки").await?;
    let rows = fetch_supplies(&s.pool, a.ctx.org_id, None, &SupplyQuery { store_id: q.store_id, from: q.from, to: q.to, supplier_id: None }).await?;
    Ok(Json(rows.into_iter().map(|x| serde_json::to_value(x).unwrap_or(Value::Null)).collect()))
}

/// Период отчётов API: даты включительно, по умолчанию — последние 30 суток.
/// Дни считаются в часовом поясе филиала, а он может опережать UTC, поэтому по умолчанию конец — завтрашняя дата UTC.
fn range(q: &DateQ) -> (NaiveDate, NaiveDate) {
    let to = q.to.unwrap_or_else(|| Utc::now().date_naive() + Duration::days(1));
    let from = q.from.unwrap_or(to - Duration::days(29));
    (from, to)
}

async fn revenue_by_day(State(s): State<AppState>, a: ApiAuth, Query(q): Query<DateQ>) -> AppResult<Json<Vec<Value>>> {
    a.check(&s, &["read:sales"], "reports.revenue", "запросил выручку по дням").await?;
    let (from, to) = range(&q);
    let rows: Vec<(Uuid, NaiveDate, f64, i64)> = sqlx::query_as(
        "SELECT s.store_id, (s.sale_date AT TIME ZONE st.timezone)::date AS day, SUM(s.total)::float8, COUNT(*) \
         FROM sales s JOIN stores st ON st.id = s.store_id \
         WHERE s.organization_id=$1 AND s.deleted_at IS NULL AND ($2::uuid IS NULL OR s.store_id=$2) \
           AND (s.sale_date AT TIME ZONE st.timezone)::date >= $3 AND (s.sale_date AT TIME ZONE st.timezone)::date <= $4 \
         GROUP BY 1, 2 ORDER BY 2, 1",
    )
    .bind(a.ctx.org_id)
    .bind(q.store_id)
    .bind(from)
    .bind(to)
    .fetch_all(&s.pool)
    .await?;
    Ok(Json(rows.into_iter().map(|(store, day, rev, n)| json!({"storeId": store, "date": day, "revenue": round2(rev), "sales": n})).collect()))
}

async fn top_rows(s: &AppState, org: Uuid, q: &DateQ, n: i64) -> AppResult<Vec<(Uuid, String, f64, f64)>> {
    let (from, to) = range(q);
    Ok(sqlx::query_as(
        "SELECT i.product_id, p.name, SUM(i.qty - i.returned_qty)::float8, SUM((i.qty - i.returned_qty) * i.price)::float8 \
         FROM sale_items i JOIN sales s ON s.id = i.sale_id JOIN products p ON p.id = i.product_id JOIN stores st ON st.id = s.store_id \
         WHERE s.organization_id=$1 AND s.deleted_at IS NULL AND ($2::uuid IS NULL OR s.store_id=$2) \
           AND (s.sale_date AT TIME ZONE st.timezone)::date >= $3 AND (s.sale_date AT TIME ZONE st.timezone)::date <= $4 \
         GROUP BY i.product_id, p.name ORDER BY 4 DESC, 2 LIMIT $5",
    )
    .bind(org)
    .bind(q.store_id)
    .bind(from)
    .bind(to)
    .bind(n)
    .fetch_all(&s.pool)
    .await?)
}

async fn sales_summary(State(s): State<AppState>, a: ApiAuth, Query(q): Query<DateQ>) -> AppResult<Json<Value>> {
    a.check(&s, &["read:sales"], "reports.summary", "запросил сводку продаж").await?;
    let (from, to) = range(&q);
    let (revenue, count): (f64, i64) = sqlx::query_as(
        "SELECT COALESCE(SUM(s.total),0)::float8, COUNT(*) FROM sales s JOIN stores st ON st.id = s.store_id \
         WHERE s.organization_id=$1 AND s.deleted_at IS NULL AND ($2::uuid IS NULL OR s.store_id=$2) \
           AND (s.sale_date AT TIME ZONE st.timezone)::date >= $3 AND (s.sale_date AT TIME ZONE st.timezone)::date <= $4",
    )
    .bind(a.ctx.org_id)
    .bind(q.store_id)
    .bind(from)
    .bind(to)
    .fetch_one(&s.pool)
    .await?;
    let best = top_rows(&s, a.ctx.org_id, &q, 5).await?;
    Ok(Json(json!({
        "from": from, "to": to, "revenue": round2(revenue), "salesCount": count,
        "avgCheck": if count > 0 { round2(revenue / count as f64) } else { 0.0 },
        "bestSellers": best.iter().map(|(id, name, qty, sum)| json!({"productId": id, "name": name, "qty": round2(*qty), "revenue": round2(*sum)})).collect::<Vec<_>>(),
    })))
}

async fn top_products(State(s): State<AppState>, a: ApiAuth, Query(q): Query<DateQ>) -> AppResult<Json<Vec<Value>>> {
    a.check(&s, &["read:sales"], "reports.top_products", "запросил рейтинг товаров").await?;
    let rows = top_rows(&s, a.ctx.org_id, &q, q.limit.unwrap_or(20).clamp(1, 200)).await?;
    Ok(Json(rows.iter().map(|(id, name, qty, sum)| json!({"productId": id, "name": name, "qty": round2(*qty), "revenue": round2(*sum)})).collect()))
}

async fn balances(s: &AppState, org: Uuid, only: Option<Uuid>) -> AppResult<Vec<Value>> {
    let rows: Vec<(Uuid, String, Uuid, f64)> = sqlx::query_as(
        "SELECT p.id, p.name, st.id, COALESCE(ss.qty,0)::float8 FROM products p CROSS JOIN stores st \
         LEFT JOIN store_stock ss ON ss.product_id = p.id AND ss.store_id = st.id \
         WHERE p.organization_id=$1 AND p.deleted_at IS NULL AND NOT p.is_service AND st.organization_id=$1 AND st.deleted_at IS NULL \
           AND ($2::uuid IS NULL OR p.id=$2) ORDER BY p.name, st.created_at",
    )
    .bind(org)
    .bind(only)
    .fetch_all(&s.pool)
    .await?;
    let mut out: Vec<Value> = Vec::new();
    for (pid, name, store, qty) in rows {
        match out.last_mut() {
            Some(last) if last["productId"] == json!(pid) => last["stores"].as_array_mut().expect("array").push(json!({"storeId": store, "qty": qty})),
            _ => out.push(json!({"productId": pid, "name": name, "stores": [{"storeId": store, "qty": qty}]})),
        }
    }
    Ok(out)
}

async fn stock_balances(State(s): State<AppState>, a: ApiAuth) -> AppResult<Json<Vec<Value>>> {
    a.check(&s, &["read:stock"], "stock.list", "запросил остатки").await?;
    Ok(Json(balances(&s, a.ctx.org_id, None).await?))
}

async fn stock_balance_one(State(s): State<AppState>, a: ApiAuth, Path(id): Path<Uuid>) -> AppResult<Json<Value>> {
    a.check(&s, &["read:stock"], "stock.get", "запросил остатки товара").await?;
    balances(&s, a.ctx.org_id, Some(id)).await?.into_iter().next().map(Json).ok_or_else(|| AppError::NotFound("Товар не найден или это услуга".into()))
}

// =========================== черновики ===========================
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DraftItem {
    pub product_id: Uuid,
    pub qty: f64,
    pub price: Option<f64>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DraftDto {
    pub store_id: Uuid,
    pub items: Vec<DraftItem>,
    pub client_phone: Option<String>,
    pub supplier_id: Option<Uuid>,
    pub comment: Option<String>,
}

async fn create_draft(s: &AppState, a: &ApiAuth, kind: &str, dto: DraftDto) -> AppResult<DraftView> {
    let (n,): (i64,) = sqlx::query_as("SELECT count(*) FROM stores WHERE id=$1 AND organization_id=$2 AND deleted_at IS NULL")
        .bind(dto.store_id)
        .bind(a.ctx.org_id)
        .fetch_one(&s.pool)
        .await?;
    if n == 0 {
        return Err(AppError::BadRequest("Филиал не найден".into()));
    }
    if dto.items.is_empty() {
        return Err(AppError::BadRequest("Добавьте позиции".into()));
    }
    let ids: Vec<Uuid> = dto.items.iter().map(|i| i.product_id).collect();
    let (found,): (i64,) = sqlx::query_as("SELECT count(DISTINCT id) FROM products WHERE organization_id=$1 AND deleted_at IS NULL AND id = ANY($2)")
        .bind(a.ctx.org_id)
        .bind(&ids)
        .fetch_one(&s.pool)
        .await?;
    let distinct: std::collections::HashSet<_> = ids.iter().collect();
    if found as usize != distinct.len() {
        return Err(AppError::BadRequest("Товар не найден".into()));
    }
    if dto.items.iter().any(|i| !(i.qty.is_finite() && i.qty > 0.0) || i.price.is_some_and(|p| !(p.is_finite() && p >= 0.0))) {
        return Err(AppError::BadRequest("Количество должно быть больше нуля, цена — не отрицательной".into()));
    }
    if kind == "acceptance" && dto.items.iter().any(|i| i.price.is_none()) {
        return Err(AppError::BadRequest("В приёмке у каждой позиции нужна закупочная цена".into()));
    }
    let id = Uuid::new_v4();
    let payload = json!({
        "items": dto.items.iter().map(|i| json!({"productId": i.product_id, "qty": i.qty, "price": i.price})).collect::<Vec<_>>(),
        "clientPhone": dto.client_phone, "supplierId": dto.supplier_id, "comment": dto.comment,
    });
    sqlx::query("INSERT INTO api_drafts (id, organization_id, token_id, kind, store_id, payload, expires_at) VALUES ($1,$2,$3,$4,$5,$6,$7)")
        .bind(id)
        .bind(a.ctx.org_id)
        .bind(a.token_id)
        .bind(kind)
        .bind(dto.store_id)
        .bind(&payload)
        .bind(Utc::now() + Duration::days(7))
        .execute(&s.pool)
        .await?;
    get_draft(s, a, id).await
}

async fn get_draft(s: &AppState, a: &ApiAuth, id: Uuid) -> AppResult<DraftView> {
    sqlx::query_as::<_, DraftView>(SELECT_DRAFTS)
        .bind(a.ctx.org_id)
        .bind(Some(id))
        .bind(None::<String>)
        .fetch_optional(&s.pool)
        .await?
        .ok_or_else(|| AppError::NotFound("Черновик не найден".into()))
}

/// Предложить продажу: ничего не продаётся, пока пользователь не откроет черновик в приложении и не проведёт его.
async fn sale_draft(State(s): State<AppState>, a: ApiAuth, Json(dto): Json<DraftDto>) -> AppResult<Json<DraftView>> {
    a.check(&s, &["draft:sales"], "drafts.sale", "предложил продажу").await?;
    Ok(Json(create_draft(&s, &a, "sale", dto).await?))
}

async fn acceptance_draft(State(s): State<AppState>, a: ApiAuth, Json(dto): Json<DraftDto>) -> AppResult<Json<DraftView>> {
    a.check(&s, &["draft:acceptances"], "drafts.acceptance", "предложил приёмку").await?;
    Ok(Json(create_draft(&s, &a, "acceptance", dto).await?))
}

async fn drafts(State(s): State<AppState>, a: ApiAuth) -> AppResult<Json<Vec<DraftView>>> {
    a.check(&s, &["draft:sales", "draft:acceptances", "read:all"], "drafts.list", "запросил черновики").await?;
    Ok(Json(sqlx::query_as::<_, DraftView>(SELECT_DRAFTS).bind(a.ctx.org_id).bind(None::<Uuid>).bind(None::<String>).fetch_all(&s.pool).await?))
}

async fn draft_one(State(s): State<AppState>, a: ApiAuth, Path(id): Path<Uuid>) -> AppResult<Json<DraftView>> {
    a.check(&s, &["draft:sales", "draft:acceptances", "read:all"], "drafts.get", "запросил черновик").await?;
    Ok(Json(get_draft(&s, &a, id).await?))
}
