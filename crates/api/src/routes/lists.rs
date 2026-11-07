//! /api/lists/{kind} — настраиваемые списки: методы оплаты/покупки, скидки, причины, категории расходов.
use crate::{
    auth::AuthContext,
    error::{AppError, AppResult},
    AppState,
};
use axum::{
    extract::{Path, State},
    routing::get,
    Json, Router,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::{FromRow, Postgres, Transaction};
use uuid::Uuid;

pub const KINDS: [&str; 8] = [
    "payment_methods",
    "purchase_methods",
    "discounts",
    "return_reasons",
    "writeoff_reasons",
    "spending_categories",
    "product_modifiers",
    "print_templates",
];

pub fn router() -> Router<AppState> {
    Router::new().route("/{kind}", get(list).post(create)).route("/{kind}/{id}", get(one).put(update).delete(remove))
}

#[derive(Serialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct ItemView {
    pub id: Uuid,
    pub name: String,
    pub params: Value,
    pub sort_order: i32,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemDto {
    pub name: String,
    #[serde(default)]
    pub params: Value,
    pub sort_order: Option<i32>,
}

fn check_kind(kind: &str) -> AppResult<()> {
    if KINDS.contains(&kind) {
        Ok(())
    } else {
        Err(AppError::NotFound("Список не найден".into()))
    }
}

/// Проверка и нормализация параметров по виду списка.
fn check_params(kind: &str, params: &Value) -> AppResult<Value> {
    let obj = params.as_object().cloned().unwrap_or_default();
    let num = |k: &str| obj.get(k).and_then(Value::as_f64);
    match kind {
        "payment_methods" => {
            let k = obj.get("kind").and_then(Value::as_str).unwrap_or("other");
            if !["cash", "qr", "terminal", "other"].contains(&k) {
                return Err(AppError::BadRequest("Неизвестный тип оплаты".into()));
            }
            let commission = num("commission").unwrap_or(0.0);
            if !(0.0..=100.0).contains(&commission) {
                return Err(AppError::BadRequest("Комиссия от 0 до 100%".into()));
            }
            Ok(json!({ "kind": k, "commission": commission }))
        }
        "purchase_methods" => {
            let m = num("markupPercent").unwrap_or(0.0);
            if !(0.0..=1000.0).contains(&m) {
                return Err(AppError::BadRequest("Надбавка от 0 до 1000%".into()));
            }
            let delivery = obj.get("delivery").and_then(Value::as_bool).unwrap_or(false);
            Ok(json!({ "markupPercent": m, "delivery": delivery }))
        }
        "discounts" => {
            let t = obj.get("type").and_then(Value::as_str).unwrap_or("percent");
            let v = num("value").ok_or_else(|| AppError::BadRequest("Укажите размер скидки".into()))?;
            if !["percent", "amount"].contains(&t) || v <= 0.0 || (t == "percent" && v > 100.0) {
                return Err(AppError::BadRequest("Некорректная скидка".into()));
            }
            Ok(json!({ "type": t, "value": v }))
        }
        // «Модификаторы товаров»: группа добавок (товары с ценой добавки) и доступность по филиалам
        "product_modifiers" => {
            let mut items = Vec::new();
            for it in obj.get("items").and_then(Value::as_array).cloned().unwrap_or_default() {
                let pid = it.get("productId").and_then(Value::as_str).and_then(|v| Uuid::parse_str(v).ok());
                let price = it.get("price").and_then(Value::as_f64).filter(|p| p.is_finite() && *p >= 0.0);
                let (Some(pid), Some(price)) = (pid, price) else {
                    return Err(AppError::BadRequest("Некорректная добавка: нужен товар и цена не меньше нуля".into()));
                };
                let name = it.get("name").and_then(Value::as_str).unwrap_or("").to_string();
                items.push(json!({ "productId": pid, "name": name, "price": price }));
            }
            let stores = obj.get("stores").and_then(Value::as_object).cloned().unwrap_or_default();
            if !stores.values().all(Value::is_boolean) {
                return Err(AppError::BadRequest("Некорректные настройки филиалов".into()));
            }
            Ok(json!({ "items": items, "stores": stores }))
        }
        // «Настройки печати»: шаблон накладной; поля хранятся как есть, но с ограничением размера
        "print_templates" => {
            if params.to_string().len() > 20_000 {
                return Err(AppError::BadRequest("Шаблон слишком большой".into()));
            }
            Ok(Value::Object(obj))
        }
        _ => Ok(json!({})),
    }
}

fn need_name(name: &str) -> AppResult<String> {
    let n = name.trim();
    if n.is_empty() {
        return Err(AppError::BadRequest("Введите название".into()));
    }
    Ok(n.to_string())
}

/// Начальные данные новой организации.
pub async fn seed_defaults(tx: &mut Transaction<'_, Postgres>, org_id: Uuid) -> AppResult<()> {
    let mut rows: Vec<(&str, String, Value)> = vec![
        ("payment_methods", "Наличные".into(), json!({"kind": "cash"})),
        ("payment_methods", "QR".into(), json!({"kind": "qr"})),
        ("payment_methods", "Терминал".into(), json!({"kind": "terminal"})),
        ("purchase_methods", "Магазин".into(), json!({"markupPercent": 0.0, "delivery": false})),
        ("purchase_methods", "Доставка".into(), json!({"markupPercent": 0.0, "delivery": true})),
        ("return_reasons", "Брак".into(), json!({})),
        ("return_reasons", "Не подошёл".into(), json!({})),
        ("return_reasons", "Другое".into(), json!({})),
        ("writeoff_reasons", "Брак".into(), json!({})),
        ("writeoff_reasons", "Истёк срок годности".into(), json!({})),
        ("writeoff_reasons", "Другое".into(), json!({})),
        ("spending_categories", "Аренда".into(), json!({})),
        ("spending_categories", "Коммунальные услуги".into(), json!({})),
        ("spending_categories", "Реклама".into(), json!({})),
        ("spending_categories", "Прочее".into(), json!({})),
    ];
    for p in (5..=50).step_by(5) {
        rows.push(("discounts", format!("{p}%"), json!({"type": "percent", "value": p as f64})));
    }
    for (i, (kind, name, params)) in rows.into_iter().enumerate() {
        sqlx::query("INSERT INTO list_items (id, organization_id, kind, name, params, sort_order) VALUES ($1,$2,$3,$4,$5,$6)")
            .bind(Uuid::new_v4())
            .bind(org_id)
            .bind(kind)
            .bind(name)
            .bind(params)
            .bind(i as i32)
            .execute(&mut **tx)
            .await?;
    }
    Ok(())
}

async fn list(State(s): State<AppState>, ctx: AuthContext, Path(kind): Path<String>) -> AppResult<Json<Vec<ItemView>>> {
    check_kind(&kind)?;
    let rows = sqlx::query_as::<_, ItemView>(
        "SELECT id, name, params, sort_order FROM list_items WHERE organization_id=$1 AND kind=$2 AND deleted_at IS NULL \
         ORDER BY sort_order, created_at",
    )
    .bind(ctx.org_id)
    .bind(&kind)
    .fetch_all(&s.pool)
    .await?;
    Ok(Json(rows))
}

async fn one(State(s): State<AppState>, ctx: AuthContext, Path((kind, id)): Path<(String, Uuid)>) -> AppResult<Json<ItemView>> {
    check_kind(&kind)?;
    let row = sqlx::query_as::<_, ItemView>(
        "SELECT id, name, params, sort_order FROM list_items WHERE id=$1 AND organization_id=$2 AND kind=$3 AND deleted_at IS NULL",
    )
    .bind(id)
    .bind(ctx.org_id)
    .bind(&kind)
    .fetch_optional(&s.pool)
    .await?
    .ok_or_else(|| AppError::NotFound("Запись не найдена".into()))?;
    Ok(Json(row))
}

async fn create(
    State(s): State<AppState>,
    ctx: AuthContext,
    Path(kind): Path<String>,
    Json(dto): Json<ItemDto>,
) -> AppResult<Json<ItemView>> {
    check_kind(&kind)?;
    ctx.require("settings.edit")?;
    let name = need_name(&dto.name)?;
    let params = check_params(&kind, &dto.params)?;
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO list_items (id, organization_id, kind, name, params, sort_order) VALUES ($1,$2,$3,$4,$5,\
         COALESCE($6,(SELECT COALESCE(max(sort_order),0)+1 FROM list_items WHERE organization_id=$2 AND kind=$3)))",
    )
    .bind(id)
    .bind(ctx.org_id)
    .bind(&kind)
    .bind(name)
    .bind(params)
    .bind(dto.sort_order)
    .execute(&s.pool)
    .await?;
    one(State(s), ctx, Path((kind, id))).await
}

async fn update(
    State(s): State<AppState>,
    ctx: AuthContext,
    Path((kind, id)): Path<(String, Uuid)>,
    Json(dto): Json<ItemDto>,
) -> AppResult<Json<ItemView>> {
    check_kind(&kind)?;
    ctx.require("settings.edit")?;
    let name = need_name(&dto.name)?;
    let params = check_params(&kind, &dto.params)?;
    let res = sqlx::query(
        "UPDATE list_items SET name=$4, params=$5, sort_order=COALESCE($6, sort_order) \
         WHERE id=$1 AND organization_id=$2 AND kind=$3 AND deleted_at IS NULL",
    )
    .bind(id)
    .bind(ctx.org_id)
    .bind(&kind)
    .bind(name)
    .bind(params)
    .bind(dto.sort_order)
    .execute(&s.pool)
    .await?;
    if res.rows_affected() == 0 {
        return Err(AppError::NotFound("Запись не найдена".into()));
    }
    one(State(s), ctx, Path((kind, id))).await
}

async fn remove(State(s): State<AppState>, ctx: AuthContext, Path((kind, id)): Path<(String, Uuid)>) -> AppResult<Json<Value>> {
    check_kind(&kind)?;
    ctx.require("settings.edit")?;
    if kind == "payment_methods" {
        let (n,): (i64,) = sqlx::query_as("SELECT count(*) FROM list_items WHERE organization_id=$1 AND kind=$2 AND deleted_at IS NULL")
            .bind(ctx.org_id)
            .bind(&kind)
            .fetch_one(&s.pool)
            .await?;
        if n <= 1 {
            return Err(AppError::BadRequest("Нельзя удалить последний способ оплаты".into()));
        }
    }
    let res = sqlx::query("UPDATE list_items SET deleted_at=now() WHERE id=$1 AND organization_id=$2 AND kind=$3 AND deleted_at IS NULL")
        .bind(id)
        .bind(ctx.org_id)
        .bind(&kind)
        .execute(&s.pool)
        .await?;
    if res.rows_affected() == 0 {
        return Err(AppError::NotFound("Запись не найдена".into()));
    }
    Ok(Json(json!({ "success": true })))
}
