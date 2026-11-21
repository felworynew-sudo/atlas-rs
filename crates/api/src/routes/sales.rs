//! /api/sales — продажи, возвраты, удаление с возвратом остатков
use crate::{
    auth::AuthContext,
    error::{AppError, AppResult},
    ledger,
    routes::products::cost_nodes,
    AppState,
};
use atlas_domain::{
    catalog::unit_cost,
    sale::{punch_card, refund_amount, round2, split_ok, stock_effects, totals, Discount, TotalsInput, DEFAULT_CASHBACK_PERCENT},
};
use axum::{
    extract::{Path, Query, State},
    routing::{get, post},
    Json, Router,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::{FromRow, PgPool};
use std::collections::HashMap;
use uuid::Uuid;

pub fn router() -> Router<AppState> {
    Router::new().route("/", get(list).post(create)).route("/{id}", get(one).patch(update).delete(remove))
        .route("/{id}/staff", get(staff_for_sale)).route("/{id}/returns", post(create_return))
}

#[derive(Serialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct SaleItemView {
    pub id: Uuid,
    #[serde(skip)]
    pub sale_id: Uuid,
    pub product_id: Option<Uuid>,
    pub name: String,
    pub qty: f64,
    pub price: f64,
    pub cost_price: f64,
    pub returned_qty: f64,
}

#[derive(Serialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct SaleView {
    pub id: Uuid,
    pub store_id: Uuid,
    pub user_id: Uuid,
    pub user_name: String,
    pub client_id: Option<Uuid>,
    pub purchase_method_id: Option<Uuid>,
    pub payment_method_id: Option<Uuid>,
    pub second_payment_method_id: Option<Uuid>,
    pub first_payment_value: Option<f64>,
    pub second_payment_value: Option<f64>,
    pub subtotal: f64,
    pub markup_amount: f64,
    pub discount_amount: f64,
    pub discount_percentage: f64,
    pub bonus_reduction: f64,
    pub bonus_accumulation: f64,
    pub total: f64,
    pub cost_total: f64,
    pub returned_amount: f64,
    pub delivery: bool,
    pub comment: String,
    pub frame_numbers: String,
    pub order_kind: String,
    pub revision: String,
    pub can_edit_amounts: bool,
    pub sale_date: DateTime<Utc>,
    pub punch_card_reduction: f64,
    pub delivery_address: String,
    pub delivery_phone: String,
    pub delivery_price: f64,
    pub order_id: Option<Uuid>,
    pub shift_id: Option<Uuid>,
    #[sqlx(skip)]
    pub items: Vec<SaleItemView>,
}

const SELECT_SALES: &str = "SELECT s.id, s.store_id, s.user_id, u.name AS user_name, s.client_id, s.purchase_method_id, \
     s.payment_method_id, s.second_payment_method_id, s.first_payment_value::float8 AS first_payment_value, \
     s.second_payment_value::float8 AS second_payment_value, s.subtotal::float8 AS subtotal, \
     s.markup_amount::float8 AS markup_amount, s.discount_amount::float8 AS discount_amount, \
     s.discount_percentage::float8 AS discount_percentage, s.bonus_reduction::float8 AS bonus_reduction, \
     s.bonus_accumulation::float8 AS bonus_accumulation, s.total::float8 AS total, s.cost_total::float8 AS cost_total, \
     s.returned_amount::float8 AS returned_amount, s.delivery, s.comment, s.frame_numbers, s.order_kind, \
     s.xmin::text AS revision, s.sale_date, \
     (s.returned_amount=0 AND s.bonus_reduction=0 AND s.bonus_accumulation=0 AND s.punch_delta=0 \
       AND s.punch_card_reduction=0 AND s.markup_amount=0 AND s.discount_amount=0 \
       AND s.second_payment_method_id IS NULL \
       AND NOT EXISTS (SELECT 1 FROM sale_returns r WHERE r.sale_id=s.id)) AS can_edit_amounts, \
     s.punch_card_reduction::float8 AS punch_card_reduction, s.delivery_address, s.delivery_phone, \
     s.delivery_price::float8 AS delivery_price, s.order_id, s.shift_id \
     FROM sales s JOIN users u ON u.id = s.user_id \
     WHERE s.organization_id=$1 AND s.deleted_at IS NULL \
       AND ($2::uuid IS NULL OR s.id=$2) AND ($3::uuid IS NULL OR s.store_id=$3) \
       AND ($4::timestamptz IS NULL OR s.sale_date >= $4) AND ($5::timestamptz IS NULL OR s.sale_date < $5) \
       AND ($6::uuid IS NULL OR s.client_id=$6) AND ($7::uuid IS NULL OR s.user_id=$7) \
       AND ($8::uuid IS NULL OR s.shift_id=$8) \
     ORDER BY s.sale_date DESC, s.created_at DESC LIMIT 500";

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ListQuery {
    pub store_id: Option<Uuid>,
    pub from: Option<DateTime<Utc>>,
    pub to: Option<DateTime<Utc>>,
    pub client_id: Option<Uuid>,
    pub user_id: Option<Uuid>,
    pub shift_id: Option<Uuid>,
}

pub async fn fetch_sales(pool: &PgPool, org: Uuid, id: Option<Uuid>, q: &ListQuery) -> AppResult<Vec<SaleView>> {
    let mut sales = sqlx::query_as::<_, SaleView>(SELECT_SALES)
        .bind(org)
        .bind(id)
        .bind(q.store_id)
        .bind(q.from)
        .bind(q.to)
        .bind(q.client_id)
        .bind(q.user_id)
        .bind(q.shift_id)
        .fetch_all(pool)
        .await?;
    let ids: Vec<Uuid> = sales.iter().map(|s| s.id).collect();
    let items = sqlx::query_as::<_, SaleItemView>(
        "SELECT id, sale_id, product_id, name, qty::float8 AS qty, price::float8 AS price, cost_price::float8 AS cost_price, \
         returned_qty::float8 AS returned_qty FROM sale_items WHERE sale_id = ANY($1) ORDER BY name, id",
    )
    .bind(&ids)
    .fetch_all(pool)
    .await?;
    let mut by_sale: HashMap<Uuid, Vec<SaleItemView>> = HashMap::new();
    for i in items {
        by_sale.entry(i.sale_id).or_default().push(i);
    }
    for s in &mut sales {
        s.items = by_sale.remove(&s.id).unwrap_or_default();
    }
    Ok(sales)
}

async fn list(State(s): State<AppState>, ctx: AuthContext, Query(q): Query<ListQuery>) -> AppResult<Json<Vec<SaleView>>> {
    ctx.require("sale.history")?;
    Ok(Json(fetch_sales(&s.pool, ctx.org_id, None, &q).await?))
}

async fn one(State(s): State<AppState>, ctx: AuthContext, Path(id): Path<Uuid>) -> AppResult<Json<SaleView>> {
    ctx.require("sale.history")?;
    fetch_sales(&s.pool, ctx.org_id, Some(id), &ListQuery::default())
        .await?
        .into_iter()
        .next()
        .map(Json)
        .ok_or_else(|| AppError::NotFound("Продажа не найдена".into()))
}

/// Доступ к филиалу: админ, право «Просмотр других филиалов» или филиал в списке сотрудника.
pub async fn check_store(pool: &PgPool, ctx: &AuthContext, store_id: Uuid) -> AppResult<()> {
    let (exists,): (i64,) = sqlx::query_as("SELECT count(*) FROM stores WHERE id=$1 AND organization_id=$2 AND deleted_at IS NULL")
        .bind(store_id)
        .bind(ctx.org_id)
        .fetch_one(pool)
        .await?;
    if exists == 0 {
        return Err(AppError::BadRequest("Филиал не найден".into()));
    }
    if ctx.is_admin() || ctx.can("settings.other_stores") {
        return Ok(());
    }
    let (ok,): (bool,) = sqlx::query_as("SELECT $2 = ANY(store_ids) FROM users WHERE id=$1")
        .bind(ctx.user_id)
        .bind(store_id)
        .fetch_one(pool)
        .await?;
    if ok {
        Ok(())
    } else {
        Err(AppError::Forbidden)
    }
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ItemDto {
    pub product_id: Option<Uuid>,
    pub name: Option<String>,
    pub qty: f64,
    pub price: Option<f64>,
    pub purchase_price: Option<f64>,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CreateSaleDto {
    pub store_id: Uuid,
    /// кассир подтвердил продажу товара, отмеченного как отсутствующий на складе («Я знаю, что делаю»)
    #[serde(default)]
    pub allow_unavailable: bool,
    pub client_id: Option<Uuid>,
    pub items: Vec<ItemDto>,
    pub price_type_id: Option<Uuid>,
    pub payment_method_id: Option<Uuid>,
    pub second_payment_method_id: Option<Uuid>,
    pub first_payment_value: Option<f64>,
    pub second_payment_value: Option<f64>,
    pub purchase_method_id: Option<Uuid>,
    pub discount_percentage: Option<f64>,
    pub discount_amount: Option<f64>,
    pub bonus_reduction: Option<f64>,
    pub cashback_enabled: Option<bool>,
    #[serde(default)]
    pub delivery: bool,
    pub comment: Option<String>,
    pub frame_numbers: Option<String>,
    pub order_kind: Option<String>,
    pub sale_date: Option<DateTime<Utc>>,
    pub shift_id: Option<Uuid>,
    pub user_id: Option<Uuid>,
    pub idempotency_key: Option<String>,
    pub delivery_address: Option<String>,
    pub delivery_phone: Option<String>,
    /// стоимость доставки; по умолчанию — из «Настроек продаж»
    pub delivery_price: Option<f64>,
    /// заказ клиента, из которого создана продажа
    pub order_id: Option<Uuid>,
    /// партия импорта (`POST /api/import/sales`)
    pub import_batch_id: Option<Uuid>,
}

struct Line {
    service: bool,
    product_id: Option<Uuid>,
    name: String,
    qty: f64,
    price: f64,
    cost: f64,
}

async fn resolve_lines(pool: &PgPool, ctx: &AuthContext, store_id: Uuid, items: &[ItemDto], allow_unavailable: bool) -> AppResult<Vec<Line>> {
    let pids: Vec<Uuid> = items.iter().filter_map(|i| i.product_id).collect();
    let prods: Vec<(Uuid, String, bool, bool, bool, f64)> = sqlx::query_as(
        "SELECT p.id, p.name, p.is_service, p.allow_fractional, COALESCE(ps.active, true), COALESCE(ps.sale_price, 0)::float8 \
         FROM products p LEFT JOIN product_stores ps ON ps.product_id = p.id AND ps.store_id = $3 \
         WHERE p.organization_id=$1 AND p.deleted_at IS NULL AND p.id = ANY($2)",
    )
    .bind(ctx.org_id)
    .bind(&pids)
    .bind(store_id)
    .fetch_all(pool)
    .await?;
    let pmap: HashMap<Uuid, _> = prods.into_iter().map(|p| (p.0, p)).collect();
    let costs = cost_nodes(pool, ctx.org_id).await?;

    let mut lines: Vec<Line> = Vec::new();
    for it in items {
        if !(it.qty.is_finite() && it.qty > 0.0) {
            return Err(AppError::BadRequest("Количество должно быть больше нуля".into()));
        }
        match it.product_id {
            Some(pid) => {
                let (_, name, service, fractional, active, store_price) =
                    pmap.get(&pid).ok_or_else(|| AppError::BadRequest("Товар не найден".into()))?;
                if !*active && !allow_unavailable {
                    return Err(AppError::BadRequest(format!("Товар «{name}» отмечен как отсутствующий на складе")));
                }
                if !*fractional && it.qty.fract() != 0.0 {
                    return Err(AppError::BadRequest(format!("Количество «{name}» должно быть целым")));
                }
                let price = it.price.unwrap_or(*store_price);
                if !(price.is_finite() && price >= 0.0) {
                    return Err(AppError::BadRequest("Цена не может быть отрицательной".into()));
                }
                lines.push(Line { service: *service, product_id: Some(pid), name: name.clone(), qty: it.qty, price, cost: unit_cost(&costs, pid) });
            }
            None => {
                ctx.require("sale.custom_product")?;
                let name = it.name.as_deref().map(str::trim).filter(|n| !n.is_empty());
                let name = name.ok_or_else(|| AppError::BadRequest("Введите название позиции".into()))?;
                let price = it.price.ok_or_else(|| AppError::BadRequest("Укажите цену позиции".into()))?;
                if !(price.is_finite() && price >= 0.0) {
                    return Err(AppError::BadRequest("Цена не может быть отрицательной".into()));
                }
                lines.push(Line { service: false, product_id: None, name: name.to_string(), qty: it.qty, price, cost: it.purchase_price.unwrap_or(0.0).max(0.0) });
            }
        }
    }
    Ok(lines)
}

async fn list_item_exists(pool: &PgPool, org: Uuid, kind: &str, id: Uuid) -> AppResult<Value> {
    let row: Option<(Value,)> = sqlx::query_as("SELECT params FROM list_items WHERE id=$1 AND organization_id=$2 AND kind=$3 AND deleted_at IS NULL")
        .bind(id)
        .bind(org)
        .bind(kind)
        .fetch_optional(pool)
        .await?;
    row.map(|r| r.0).ok_or_else(|| AppError::BadRequest("Значение справочника не найдено".into()))
}

async fn first_of_kind(pool: &PgPool, org: Uuid, kind: &str) -> AppResult<Option<Uuid>> {
    let row: Option<(Uuid,)> = sqlx::query_as(
        "SELECT id FROM list_items WHERE organization_id=$1 AND kind=$2 AND deleted_at IS NULL ORDER BY sort_order, created_at LIMIT 1",
    )
    .bind(org)
    .bind(kind)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|r| r.0))
}

pub async fn create(State(s): State<AppState>, ctx: AuthContext, Json(dto): Json<CreateSaleDto>) -> AppResult<Json<SaleView>> {
    ctx.require("sale.new")?;
    if dto.items.is_empty() {
        return Err(AppError::BadRequest("Добавьте товар, чтобы оформить продажу".into()));
    }
    // повтор запроса с тем же ключом возвращает уже созданную продажу
    if let Some(key) = dto.idempotency_key.as_deref().filter(|k| !k.is_empty()) {
        let row: Option<(Uuid,)> = sqlx::query_as("SELECT id FROM sales WHERE organization_id=$1 AND idempotency_key=$2")
            .bind(ctx.org_id)
            .bind(key)
            .fetch_optional(&s.pool)
            .await?;
        if let Some((id,)) = row {
            return created_view(&s.pool, ctx.org_id, id).await;
        }
    }
    check_store(&s.pool, &ctx, dto.store_id).await?;

    // при включённом модуле «Смена» продажа привязывается к открытой смене; без неё — только по праву
    let price_type: Option<Uuid> = match dto.price_type_id {
        Some(id) => {
            let (n,): (i64,) = sqlx::query_as("SELECT count(*) FROM price_types WHERE id=$1 AND organization_id=$2 AND deleted_at IS NULL")
                .bind(id)
                .bind(ctx.org_id)
                .fetch_one(&s.pool)
                .await?;
            if n == 0 {
                return Err(AppError::BadRequest("Тип цены не найден".into()));
            }
            Some(id)
        }
        None => None,
    };
    // позиции
    let lines = resolve_lines(&s.pool, &ctx, dto.store_id, &dto.items, dto.allow_unavailable).await?;
    let snodes = ledger::stock_nodes(&s.pool, ctx.org_id).await?;
    let subtotal: f64 = lines.iter().map(|l| l.qty * l.price).sum();
    let cost_total: f64 = lines.iter().map(|l| l.qty * l.cost).sum();

    // методы оплаты и покупки
    let payment_id = match dto.payment_method_id {
        Some(id) => {
            list_item_exists(&s.pool, ctx.org_id, "payment_methods", id).await?;
            Some(id)
        }
        None => first_of_kind(&s.pool, ctx.org_id, "payment_methods").await?,
    };
    if let Some(id) = dto.second_payment_method_id {
        list_item_exists(&s.pool, ctx.org_id, "payment_methods", id).await?;
    }
    let purchase_id = match dto.purchase_method_id {
        Some(id) => Some(id),
        None => first_of_kind(&s.pool, ctx.org_id, "purchase_methods").await?,
    };
    let markup_percent = match purchase_id {
        Some(id) => list_item_exists(&s.pool, ctx.org_id, "purchase_methods", id)
            .await?
            .get("markupPercent")
            .and_then(Value::as_f64)
            .unwrap_or(0.0),
        None => 0.0,
    };

    // настройки лояльности и продаж организации
    let (loyalty, sale_settings): (Value, Value) =
        sqlx::query_as("SELECT loyalty, sale_settings FROM organizations WHERE id=$1").bind(ctx.org_id).fetch_one(&s.pool).await?;
    let cashback_on = loyalty.get("cashbackEnabled").and_then(Value::as_bool).unwrap_or(true);
    let punch_on = loyalty.get("punchCardEnabled").and_then(Value::as_bool).unwrap_or(false);
    let punch_capacity = loyalty.get("punchCardCapacity").and_then(Value::as_i64).unwrap_or(7) as i32;
    let delivery_fee = if dto.delivery && sale_settings.get("deliveryPaidByCustomer").and_then(Value::as_bool).unwrap_or(true) {
        dto.delivery_price.or_else(|| sale_settings.get("deliveryPrice").and_then(Value::as_f64)).unwrap_or(0.0).max(0.0)
    } else {
        0.0
    };

    // скидка
    let explicit = match (dto.discount_percentage.filter(|v| *v > 0.0), dto.discount_amount.filter(|v| *v > 0.0)) {
        (Some(_), Some(_)) => return Err(AppError::BadRequest("Укажите одну скидку: процент или сумму".into())),
        (Some(p), None) => {
            ctx.require("sale.apply_discount")?;
            Discount::Percent(p)
        }
        (None, Some(a)) => {
            ctx.require("sale.apply_discount")?;
            Discount::Amount(a)
        }
        _ => Discount::None,
    };

    let mut tx = s.pool.begin().await?;
    let sale_date = dto.sale_date.unwrap_or_else(Utc::now);
    let seller_id = dto.user_id.unwrap_or(ctx.user_id);
    if dto.shift_id.is_some() && seller_id != ctx.user_id {
        ctx.require("shift.edit_data")?;
    } else if seller_id != ctx.user_id {
        return Err(AppError::Forbidden);
    }
    let shift_id = if ctx.module_enabled(atlas_domain::modules::Module::Shifts) {
        let id: Uuid = sqlx::query_scalar(
            "SELECT sh.id FROM shifts sh JOIN stores st ON st.id=sh.store_id \
             WHERE sh.organization_id=$1 AND sh.store_id=$2 \
               AND (sh.opened_at AT TIME ZONE st.timezone)::date=($3::timestamptz AT TIME ZONE st.timezone)::date \
               AND (CASE WHEN $4::uuid IS NULL THEN sh.closed_at IS NULL AND \
                           (st.external_id IS NULL OR ($3::timestamptz AT TIME ZONE st.timezone)::date=(now() AT TIME ZONE st.timezone)::date) \
                         ELSE sh.id=$4 AND sh.closed_at IS NOT NULL \
                           AND EXISTS (SELECT 1 FROM shift_staff ss WHERE ss.shift_id=sh.id AND ss.user_id=$5) END) \
             FOR UPDATE OF sh",
        ).bind(ctx.org_id).bind(dto.store_id).bind(sale_date).bind(dto.shift_id).bind(seller_id)
            .fetch_optional(&mut *tx).await?
            .ok_or_else(|| AppError::BadRequest(if dto.shift_id.is_some() {
                "Выберите сотрудника этой закрытой смены и укажите дату смены".into()
            } else {
                "Для продажи сначала откройте смену этого филиала на дату продажи".into()
            }))?;
        Some(id)
    } else {
        None
    };

    // клиент и бонусы
    let bonus_reduction = dto.bonus_reduction.unwrap_or(0.0);
    let (client_balance, cashback, punches_before, client_discount) = match dto.client_id {
        Some(cid) => {
            let row: Option<(f64, Option<f64>, i32, Option<Uuid>)> = sqlx::query_as(
                "SELECT bonus_balance::float8, bonus_level::float8, punch_punches, discount_id FROM clients \
                 WHERE id=$1 AND organization_id=$2 AND deleted_at IS NULL FOR UPDATE",
            )
            .bind(cid)
            .bind(ctx.org_id)
            .fetch_optional(&mut *tx)
            .await?;
            let (bal, level, punches, disc) = row.ok_or_else(|| AppError::BadRequest("Клиент не найден".into()))?;
            let on = cashback_on && dto.cashback_enabled.unwrap_or(true);
            (bal, on.then_some(level.unwrap_or(DEFAULT_CASHBACK_PERCENT)), punches, disc)
        }
        None => {
            if bonus_reduction > 0.0 {
                return Err(AppError::BadRequest("Бонусы доступны только клиенту".into()));
            }
            (0.0, None, 0, None)
        }
    };
    // постоянная скидка клиента действует, если скидка в чеке не задана вручную
    let discount = match (explicit, client_discount) {
        (Discount::None, Some(did)) => {
            let row: Option<(Value,)> = sqlx::query_as("SELECT params FROM list_items WHERE id=$1 AND kind='discounts' AND deleted_at IS NULL")
                .bind(did)
                .fetch_optional(&mut *tx)
                .await?;
            match row {
                Some((p,)) => {
                    let v = p.get("value").and_then(Value::as_f64).unwrap_or(0.0);
                    match p.get("type").and_then(Value::as_str) {
                        Some("amount") => Discount::Amount(v.min(subtotal)),
                        _ => Discount::Percent(v),
                    }
                }
                None => Discount::None,
            }
        }
        (d, _) => d,
    };
    // карта «N-й товар в подарок»
    let (punch_reduction, punches_after) = if punch_on && dto.client_id.is_some() {
        let mut units: Vec<f64> = Vec::new();
        for l in lines.iter().filter(|l| l.product_id.is_some() && !l.service) {
            units.extend(std::iter::repeat(l.price).take(l.qty.floor() as usize));
        }
        punch_card(punches_before, punch_capacity, &units)
    } else {
        (0.0, punches_before)
    };
    let t = totals(&TotalsInput {
        subtotal,
        markup_percent,
        discount,
        bonus_reduction,
        client_balance,
        cashback_percent: cashback,
        delivery_fee,
        punch_card_reduction: punch_reduction,
    })
    .map_err(AppError::BadRequest)?;

    // смешанная оплата
    let (first_value, second_value) = match dto.second_payment_method_id {
        Some(_) => {
            let (a, b) = (dto.first_payment_value.unwrap_or(0.0), dto.second_payment_value.unwrap_or(0.0));
            if !split_ok(t.total, a, b) {
                return Err(AppError::BadRequest("Сумма платежей не равна итогу чека".into()));
            }
            (Some(a), Some(b))
        }
        None => (None, None),
    };

    let sale_id = Uuid::new_v4();
    let (disc_pct, disc_amount) = match discount {
        Discount::Percent(p) => (p, t.discount),
        _ => (0.0, t.discount),
    };
    sqlx::query(
        "INSERT INTO sales (id, organization_id, store_id, user_id, client_id, purchase_method_id, payment_method_id, \
         second_payment_method_id, first_payment_value, second_payment_value, subtotal, markup_amount, discount_amount, \
         discount_percentage, bonus_reduction, bonus_accumulation, total, cost_total, delivery, comment, idempotency_key, sale_date, \
         punch_card_reduction, punch_delta, delivery_address, delivery_phone, delivery_price, order_id, import_batch_id, shift_id, price_type_id, frame_numbers, order_kind) \
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9::float8,$10::float8,$11::float8,$12::float8,$13::float8,$14::float8,$15::float8,\
         $16::float8,$17::float8,$18::float8,$19,$20,$21,COALESCE($22, now()),$23::float8,$24,$25,$26,$27::float8,$28,$29,$30,$31,$32,$33)",
    )
    .bind(sale_id)
    .bind(ctx.org_id)
    .bind(dto.store_id)
    .bind(seller_id)
    .bind(dto.client_id)
    .bind(purchase_id)
    .bind(payment_id)
    .bind(dto.second_payment_method_id)
    .bind(first_value)
    .bind(second_value)
    .bind(t.subtotal)
    .bind(t.markup)
    .bind(t.discount.max(disc_amount))
    .bind(disc_pct)
    .bind(t.bonus_reduction)
    .bind(t.accumulation)
    .bind(t.total)
    .bind((cost_total * 10_000.0).round() / 10_000.0)
    .bind(dto.delivery)
    .bind(dto.comment.unwrap_or_default())
    .bind(dto.idempotency_key.filter(|k| !k.is_empty()))
    .bind(sale_date)
    .bind(t.punch_card_reduction)
    .bind(punches_after - punches_before)
    .bind(dto.delivery_address.unwrap_or_default())
    .bind(dto.delivery_phone.unwrap_or_default())
    .bind(t.delivery_fee)
    .bind(dto.order_id)
    .bind(dto.import_batch_id)
    .bind(shift_id)
    .bind(price_type)
    .bind(dto.frame_numbers.unwrap_or_default().trim().chars().take(100).collect::<String>())
    .bind(dto.order_kind.unwrap_or_default().trim().chars().take(60).collect::<String>())
    .execute(&mut *tx)
    .await?;

    for l in &lines {
        sqlx::query(
            "INSERT INTO sale_items (id, sale_id, product_id, name, qty, price, cost_price) \
             VALUES ($1,$2,$3,$4,$5::float8,$6::float8,$7::float8)",
        )
        .bind(Uuid::new_v4())
        .bind(sale_id)
        .bind(l.product_id)
        .bind(&l.name)
        .bind(l.qty)
        .bind(l.price)
        .bind(l.cost)
        .execute(&mut *tx)
        .await?;
        if let Some(pid) = l.product_id {
            for (eid, eq) in stock_effects(&snodes, pid, l.qty) {
                let after = ledger::apply(&mut tx, ctx.org_id, dto.store_id, eid, "sale", -eq, Some(sale_id), ctx.user_id).await?;
                if after < -1e-9 && !ctx.can("sale.out_of_stock") {
                    let (name,): (String,) = sqlx::query_as("SELECT name FROM products WHERE id=$1").bind(eid).fetch_one(&mut *tx).await?;
                    return Err(AppError::BadRequest(format!("Недостаточно товара: {name}")));
                }
            }
        }
    }

    if let Some(cid) = dto.client_id {
        sqlx::query("UPDATE clients SET bonus_balance = bonus_balance - $2::float8 + $3::float8, punch_punches = $4 WHERE id=$1")
            .bind(cid)
            .bind(t.bonus_reduction)
            .bind(t.accumulation)
            .bind(punches_after)
            .execute(&mut *tx)
            .await?;
    }
    settle_sale(&mut tx, ctx.org_id, ctx.user_id, sale_id, dto.store_id, 1.0, "sale").await?;
    if let Some(id) = dto.shift_id {
        crate::routes::shifts::refresh_closed_shift(&mut tx, ctx.org_id, id).await?;
    }
    tx.commit().await?;
    created_view(&s.pool, ctx.org_id, sale_id).await
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct UpdateSaleDto {
    revision: String,
    user_id: Option<Uuid>,
    sale_date: DateTime<Utc>,
    comment: String,
    frame_numbers: String,
    order_kind: String,
    payment_method_id: Option<Uuid>,
    items: Option<Vec<ItemDto>>,
}

#[derive(Serialize, FromRow)]
struct SaleStaff {
    id: Uuid,
    name: String,
}

const SELECT_SALE_STAFF: &str =
    "SELECT u.id, u.name FROM users u JOIN roles r ON r.id=u.role_id AND r.organization_id=u.organization_id \
     WHERE u.organization_id=$1 AND u.active AND r.deleted_at IS NULL \
       AND (r.key='admin' OR 'sale.new'=ANY(r.permissions)) \
       AND (r.key='admin' OR 'settings.other_stores'=ANY(r.permissions) OR $2=ANY(u.store_ids)) \
       AND ($3::uuid IS NULL OR u.id=$3) ORDER BY u.name, u.id FOR SHARE OF u, r";

async fn staff_for_sale(
    State(s): State<AppState>, ctx: AuthContext, Path(id): Path<Uuid>,
) -> AppResult<Json<Vec<SaleStaff>>> {
    ctx.require("sale.history")?;
    ctx.require("sale.new")?;
    let store_id: Uuid = sqlx::query_scalar("SELECT store_id FROM sales WHERE id=$1 AND organization_id=$2 AND deleted_at IS NULL")
        .bind(id).bind(ctx.org_id).fetch_optional(&s.pool).await?
        .ok_or_else(|| AppError::NotFound("Продажа не найдена".into()))?;
    check_store(&s.pool, &ctx, store_id).await?;
    let staff = sqlx::query_as(SELECT_SALE_STAFF)
        .bind(ctx.org_id).bind(store_id).bind(None::<Uuid>).fetch_all(&s.pool).await?;
    Ok(Json(staff))
}

async fn update(
    State(s): State<AppState>, ctx: AuthContext, Path(id): Path<Uuid>, Json(dto): Json<UpdateSaleDto>,
) -> AppResult<Json<SaleView>> {
    ctx.require("sale.history")?;
    ctx.require("sale.new")?;
    let sale = created_view(&s.pool, ctx.org_id, id).await?.0;
    check_store(&s.pool, &ctx, sale.store_id).await?;
    let payment_changed = dto.payment_method_id != sale.payment_method_id;
    if dto.items.is_some() && !sale.can_edit_amounts {
        return Err(AppError::BadRequest("Сумму и позиции этого чека менять нельзя: есть возврат, скидка, бонусы или смешанная оплата".into()));
    }
    if payment_changed {
        let payment = dto.payment_method_id.ok_or_else(|| AppError::BadRequest("Выберите способ оплаты".into()))?;
        list_item_exists(&s.pool, ctx.org_id, "payment_methods", payment).await?;
    }
    let lines = if let Some(items) = dto.items {
        if items.is_empty() {
            return Err(AppError::BadRequest("В продаже должна остаться хотя бы одна позиция".into()));
        }
        Some(resolve_lines(&s.pool, &ctx, sale.store_id, &items, false).await?)
    } else {
        None
    };
    let stock_nodes = if lines.is_some() {
        ledger::stock_nodes(&s.pool, ctx.org_id).await?
    } else {
        HashMap::new()
    };
    let mut tx = s.pool.begin().await?;
    let locked: Option<(String, bool, Option<Uuid>)> = sqlx::query_as(
        "SELECT s.xmin::text, s.shift_id IS NULL OR (s.sale_date AT TIME ZONE st.timezone)::date = ($3::timestamptz AT TIME ZONE st.timezone)::date, s.shift_id \
         FROM sales s JOIN stores st ON st.id=s.store_id WHERE s.id=$1 AND s.organization_id=$2 AND s.deleted_at IS NULL FOR UPDATE OF s",
    )
    .bind(id).bind(ctx.org_id).bind(dto.sale_date).fetch_optional(&mut *tx).await?;
    let (revision, correct_day, shift_id) = locked.ok_or_else(|| AppError::NotFound("Продажа не найдена".into()))?;
    if revision != dto.revision || revision != sale.revision {
        return Err(AppError::Conflict("Продажа изменилась. Откройте её заново перед сохранением".into()));
    }
    if !correct_day {
        return Err(AppError::BadRequest("Для продажи, связанной со сменой, можно изменить время в пределах того же дня".into()));
    }
    let seller_id = dto.user_id.unwrap_or(sale.user_id);
    // Смена берётся из самой продажи: правку в старой закрытой смене ограничиваем всегда.
    if let Some(shift) = shift_id {
        let closed_at: Option<DateTime<Utc>> = sqlx::query_scalar(
            "SELECT closed_at FROM shifts WHERE id=$1 AND organization_id=$2",
        )
        .bind(shift).bind(ctx.org_id).fetch_optional(&mut *tx).await?
        .flatten();
        if closed_at.is_some() {
            crate::routes::shifts::ensure_closed_shift_editable(&ctx, closed_at, "исправление заказов")?;
        }
    }
    let new_seller = if seller_id != sale.user_id {
        Some(sqlx::query_as::<_, SaleStaff>(SELECT_SALE_STAFF)
            .bind(ctx.org_id).bind(sale.store_id).bind(seller_id).fetch_optional(&mut *tx).await?
            .ok_or_else(|| AppError::BadRequest("Выберите действующего сотрудника с доступом к продажам в этом филиале".into()))?)
    } else {
        None
    };

    if lines.is_some() || payment_changed {
        crate::routes::finance::reverse_tx(&mut tx, id, "sale", ctx.user_id).await?;
    }
    if let Some(lines) = &lines {
        // Сторнируем прежний чистый эффект. Движения сохраняются в журнале;
        // при следующих правках или удалении учитывается их итог.
        let net: Vec<(Uuid, f64)> = sqlx::query_as(
            "SELECT product_id, SUM(delta)::float8 FROM product_movements WHERE ref_id=$1 AND kind='sale' GROUP BY product_id ORDER BY product_id",
        ).bind(id).fetch_all(&mut *tx).await?;
        for (product, qty) in net {
            if qty.abs() > 1e-9 {
                ledger::apply(&mut tx, ctx.org_id, sale.store_id, product, "sale", -qty, Some(id), ctx.user_id).await?;
            }
        }
        sqlx::query("DELETE FROM sale_items WHERE sale_id=$1").bind(id).execute(&mut *tx).await?;
        for line in lines {
            sqlx::query("INSERT INTO sale_items (id,sale_id,product_id,name,qty,price,cost_price) VALUES ($1,$2,$3,$4,$5::float8,$6::float8,$7::float8)")
                .bind(Uuid::new_v4()).bind(id).bind(line.product_id).bind(&line.name)
                .bind(line.qty).bind(line.price).bind(line.cost).execute(&mut *tx).await?;
            if let Some(product) = line.product_id {
                for (ingredient, qty) in stock_effects(&stock_nodes, product, line.qty) {
                    let after = ledger::apply(&mut tx, ctx.org_id, sale.store_id, ingredient, "sale", -qty, Some(id), ctx.user_id).await?;
                    if after < -1e-9 && !ctx.can("sale.out_of_stock") {
                        return Err(AppError::BadRequest(format!("Недостаточно товара для позиции «{}»", line.name)));
                    }
                }
            }
        }
    }
    let subtotal = lines.as_ref().map(|lines| round2(lines.iter().map(|l| l.qty * l.price).sum())).unwrap_or(sale.subtotal);
    let cost_total = lines.as_ref().map(|lines| lines.iter().map(|l| l.qty * l.cost).sum()).unwrap_or(sale.cost_total);
    let total = if lines.is_some() { round2(subtotal + sale.delivery_price) } else { sale.total };
    sqlx::query("UPDATE sales SET sale_date=$2, comment=$3, frame_numbers=$4, order_kind=$5, payment_method_id=$6, subtotal=$7::float8, total=$8::float8, cost_total=$9::float8, user_id=$10 WHERE id=$1")
        .bind(id).bind(dto.sale_date).bind(dto.comment.trim())
        .bind(dto.frame_numbers.trim().chars().take(100).collect::<String>())
        .bind(dto.order_kind.trim().chars().take(60).collect::<String>())
        .bind(dto.payment_method_id).bind(subtotal).bind(total).bind(cost_total).bind(seller_id).execute(&mut *tx).await?;
    if lines.is_some() || payment_changed {
        settle_sale(&mut tx, ctx.org_id, ctx.user_id, id, sale.store_id, 1.0, "sale").await?;
        if let Some(id) = shift_id {
            crate::routes::shifts::refresh_closed_shift(&mut tx, ctx.org_id, id).await?;
        }
    }
    tx.commit().await?;
    let mut event = format!("Исправлена продажа {}", &id.simple().to_string()[..8]);
    if let Some(seller) = new_seller {
        event.push_str(&format!(": сотрудник «{}» → «{}»", sale.user_name, seller.name));
    }
    crate::events::log(&s.pool, ctx.org_id, Some(sale.store_id), ctx.user_id, "sale_updated", &event).await;
    created_view(&s.pool, ctx.org_id, id).await
}

/// Ответ на создание не зависит от права «История продаж».
async fn created_view(pool: &PgPool, org: Uuid, id: Uuid) -> AppResult<Json<SaleView>> {
    fetch_sales(pool, org, Some(id), &ListQuery::default())
        .await?
        .into_iter()
        .next()
        .map(Json)
        .ok_or_else(|| AppError::NotFound("Продажа не найдена".into()))
}

/// Зачисление (или возврат) выручки продажи на счета методов оплаты. Счёт задаётся в параметрах метода: `wallets{<филиал>: <счёт>}`;
/// без счёта проводка не создаётся. `scale` — доля суммы продажи (1.0 — вся, для возврата — доля возвращаемой суммы).
async fn settle_sale(tx: &mut sqlx::Transaction<'_, sqlx::Postgres>, org: Uuid, user: Uuid, sale_id: Uuid, store: Uuid, scale: f64, kind: &str) -> AppResult<()> {
    let row: Option<(Option<Uuid>, Option<Uuid>, Option<f64>, Option<f64>, f64)> = sqlx::query_as(
        "SELECT payment_method_id, second_payment_method_id, first_payment_value::float8, second_payment_value::float8, total::float8 FROM sales WHERE id=$1",
    )
    .bind(sale_id)
    .fetch_optional(&mut **tx)
    .await?;
    let Some((m1, m2, v1, v2, total)) = row else { return Ok(()) };
    let mut parts: Vec<(Uuid, f64)> = Vec::new();
    match (m1, m2) {
        (Some(a), Some(b)) => {
            parts.push((a, v1.unwrap_or(0.0)));
            parts.push((b, v2.unwrap_or(0.0)));
        }
        (Some(a), None) => parts.push((a, total)),
        _ => {}
    }
    for (method, amount) in parts {
        let amt = round2(amount * scale);
        if amt.abs() < 0.005 {
            continue;
        }
        let w: Option<(Option<String>,)> = sqlx::query_as("SELECT params->'wallets'->>$2 FROM list_items WHERE id=$1 AND organization_id=$3")
            .bind(method)
            .bind(store.to_string())
            .bind(org)
            .fetch_optional(&mut **tx)
            .await?;
        let Some(wallet) = w.and_then(|r| r.0).and_then(|t| Uuid::parse_str(&t).ok()) else { continue };
        let (ok,): (i64,) = sqlx::query_as("SELECT count(*) FROM wallets WHERE id=$1 AND organization_id=$2 AND deleted_at IS NULL")
            .bind(wallet)
            .bind(org)
            .fetch_one(&mut **tx)
            .await?;
        if ok == 0 {
            continue;
        }
        let comment = if kind == "sale" { "Выручка от продажи" } else { "Возврат по продаже" };
        crate::routes::finance::add_tx(tx, org, wallet, amt, kind, Some(sale_id), comment, user).await?;
    }
    Ok(())
}

pub async fn remove(State(s): State<AppState>, ctx: AuthContext, Path(id): Path<Uuid>) -> AppResult<Json<Value>> {
    ctx.require("sale.delete")?;
    let mut tx = s.pool.begin().await?;
    let sale: Option<(Uuid, Option<Uuid>, f64, f64, i32)> = sqlx::query_as(
        "SELECT store_id, client_id, bonus_reduction::float8, bonus_accumulation::float8, punch_delta FROM sales \
         WHERE id=$1 AND organization_id=$2 AND deleted_at IS NULL FOR UPDATE",
    )
    .bind(id)
    .bind(ctx.org_id)
    .fetch_optional(&mut *tx)
    .await?;
    let (store_id, client_id, reduction, accumulation, punch_delta) =
        sale.ok_or_else(|| AppError::NotFound("Продажа не найдена".into()))?;

    // сторно: обратное движение на чистый эффект продажи и возвратов
    let net: Vec<(Uuid, f64)> = sqlx::query_as(
        "SELECT product_id, SUM(delta)::float8 FROM product_movements WHERE ref_id=$1 AND kind IN ('sale','return') GROUP BY product_id",
    )
    .bind(id)
    .fetch_all(&mut *tx)
    .await?;
    for (pid, sum) in net {
        if sum.abs() > 1e-9 {
            ledger::apply(&mut tx, ctx.org_id, store_id, pid, "sale_deleted", -sum, Some(id), ctx.user_id).await?;
        }
    }
    if let Some(cid) = client_id {
        sqlx::query(
            "UPDATE clients SET bonus_balance = GREATEST(0, bonus_balance + $2::float8 - $3::float8), \
             punch_punches = GREATEST(0, punch_punches - $4) WHERE id=$1",
        )
            .bind(cid)
            .bind(reduction)
            .bind(accumulation)
            .bind(punch_delta)
            .execute(&mut *tx)
            .await?;
    }
    sqlx::query("UPDATE sales SET deleted_at=now() WHERE id=$1").bind(id).execute(&mut *tx).await?;
    crate::routes::finance::reverse_tx(&mut tx, id, "sale", ctx.user_id).await?;
    crate::routes::finance::reverse_tx(&mut tx, id, "sale_return", ctx.user_id).await?;
    tx.commit().await?;
    crate::events::log(&s.pool, ctx.org_id, Some(store_id), ctx.user_id, "sale_deleted", &format!("Удалена продажа {}", &id.simple().to_string()[..8])).await;
    Ok(Json(json!({ "success": true })))
}

// ---------- возврат ----------
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReturnLineDto {
    pub sale_item_id: Uuid,
    pub qty: f64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReturnDto {
    pub items: Vec<ReturnLineDto>,
    /// money — возврат денег, exchange — замена товара
    pub return_type: String,
    /// true — «Добавить обратно» на склад, false — «Списать»
    pub restock: bool,
    pub reason_id: Option<Uuid>,
    pub comment: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReturnView {
    pub id: Uuid,
    pub sale_id: Uuid,
    pub return_type: String,
    pub restock: bool,
    pub amount: f64,
}

async fn create_return(
    State(s): State<AppState>,
    ctx: AuthContext,
    Path(id): Path<Uuid>,
    Json(dto): Json<ReturnDto>,
) -> AppResult<Json<ReturnView>> {
    ctx.require("sale.history")?;
    if !["money", "exchange"].contains(&dto.return_type.as_str()) {
        return Err(AppError::BadRequest("Неизвестный способ возврата".into()));
    }
    if dto.items.is_empty() {
        return Err(AppError::BadRequest("Выберите позиции для возврата".into()));
    }
    if let Some(r) = dto.reason_id {
        list_item_exists(&s.pool, ctx.org_id, "return_reasons", r).await?;
    }
    let snodes = ledger::stock_nodes(&s.pool, ctx.org_id).await?;

    let mut tx = s.pool.begin().await?;
    let sale: Option<(Uuid, f64, f64, f64)> = sqlx::query_as(
        "SELECT store_id, subtotal::float8, total::float8, returned_amount::float8 FROM sales \
         WHERE id=$1 AND organization_id=$2 AND deleted_at IS NULL FOR UPDATE",
    )
    .bind(id)
    .bind(ctx.org_id)
    .fetch_optional(&mut *tx)
    .await?;
    let (store_id, subtotal, total, returned_amount) = sale.ok_or_else(|| AppError::NotFound("Продажа не найдена".into()))?;
    check_store(&s.pool, &ctx, store_id).await?;

    let rows: Vec<(Uuid, Option<Uuid>, f64, f64, f64)> = sqlx::query_as(
        "SELECT id, product_id, qty::float8, price::float8, returned_qty::float8 FROM sale_items WHERE sale_id=$1 FOR UPDATE",
    )
    .bind(id)
    .fetch_all(&mut *tx)
    .await?;
    let by_id: HashMap<Uuid, _> = rows.into_iter().map(|r| (r.0, r)).collect();

    let mut seen = std::collections::HashSet::new();
    let mut priced: Vec<(f64, f64)> = Vec::new();
    for l in &dto.items {
        if !seen.insert(l.sale_item_id) {
            return Err(AppError::BadRequest("Позиция указана дважды".into()));
        }
        let (_, _, qty, price, already) =
            by_id.get(&l.sale_item_id).ok_or_else(|| AppError::BadRequest("Позиция не найдена в продаже".into()))?;
        if !(l.qty.is_finite() && l.qty > 0.0) || l.qty > qty - already + 1e-9 {
            return Err(AppError::BadRequest("Нельзя вернуть больше, чем продано".into()));
        }
        priced.push((l.qty, *price));
    }
    let amount = refund_amount(subtotal, total, &priced).min(round2(total - returned_amount)).max(0.0);

    let return_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO sale_returns (id, organization_id, sale_id, user_id, return_type, restock, reason_id, comment, amount) \
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9::float8)",
    )
    .bind(return_id)
    .bind(ctx.org_id)
    .bind(id)
    .bind(ctx.user_id)
    .bind(&dto.return_type)
    .bind(dto.restock)
    .bind(dto.reason_id)
    .bind(dto.comment.unwrap_or_default())
    .bind(amount)
    .execute(&mut *tx)
    .await?;
    for l in &dto.items {
        let (_, product_id, ..) = by_id[&l.sale_item_id];
        sqlx::query("INSERT INTO sale_return_items (return_id, sale_item_id, qty) VALUES ($1,$2,$3::float8)")
            .bind(return_id)
            .bind(l.sale_item_id)
            .bind(l.qty)
            .execute(&mut *tx)
            .await?;
        sqlx::query("UPDATE sale_items SET returned_qty = returned_qty + $2::float8 WHERE id=$1")
            .bind(l.sale_item_id)
            .bind(l.qty)
            .execute(&mut *tx)
            .await?;
        if let (true, Some(pid)) = (dto.restock, product_id) {
            for (eid, eq) in stock_effects(&snodes, pid, l.qty) {
                ledger::apply(&mut tx, ctx.org_id, store_id, eid, "return", eq, Some(id), ctx.user_id).await?;
            }
        }
    }
    sqlx::query("UPDATE sales SET returned_amount = returned_amount + $2::float8 WHERE id=$1")
        .bind(id)
        .bind(amount)
        .execute(&mut *tx)
        .await?;
    if total > 0.0 && amount > 0.0 {
        settle_sale(&mut tx, ctx.org_id, ctx.user_id, id, store_id, -(amount / total), "sale_return").await?;
    }
    tx.commit().await?;
    Ok(Json(ReturnView { id: return_id, sale_id: id, return_type: dto.return_type, restock: dto.restock, amount }))
}
