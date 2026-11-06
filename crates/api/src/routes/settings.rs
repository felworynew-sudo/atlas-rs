//! Настройки организации: /api/loyalty-settings, /api/sale-settings, /api/marketplace-settings
use crate::{
    auth::AuthContext,
    error::{AppError, AppResult},
    AppState,
};
use axum::{extract::State, routing::get, Json, Router};
use serde_json::{Map, Value};
use uuid::Uuid;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/loyalty-settings", get(loyalty_get).patch(loyalty_patch))
        .route("/api/sale-settings", get(sale_get).patch(sale_patch))
        .route("/api/marketplace-settings", get(market_get).patch(market_patch))
}

enum Kind {
    Bool,
    Num(f64, f64),
    Text,
    OptUuid,
    BoolMap,
}

/// Применяет `patch` к `current`: допустимы только известные ключи, значения проверяются по типу.
fn apply(current: Value, patch: &Value, spec: &[(&str, Kind)]) -> AppResult<Value> {
    let mut out: Map<String, Value> = current.as_object().cloned().unwrap_or_default();
    let body = patch.as_object().ok_or_else(|| AppError::BadRequest("Ожидается объект настроек".into()))?;
    for (key, val) in body {
        let kind = spec
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, k)| k)
            .ok_or_else(|| AppError::BadRequest(format!("Неизвестная настройка «{key}»")))?;
        let ok = match kind {
            Kind::Bool => val.is_boolean(),
            Kind::Num(lo, hi) => val.as_f64().is_some_and(|n| n >= *lo && n <= *hi),
            Kind::Text => val.is_string(),
            Kind::OptUuid => val.is_null() || val.as_str().is_some_and(|s| Uuid::parse_str(s).is_ok()),
            Kind::BoolMap => val.as_object().is_some_and(|m| m.values().all(Value::is_boolean)),
        };
        if !ok {
            return Err(AppError::BadRequest(format!("Некорректное значение настройки «{key}»")));
        }
        out.insert(key.clone(), val.clone());
    }
    Ok(Value::Object(out))
}

async fn loyalty_get(State(s): State<AppState>, ctx: AuthContext) -> AppResult<Json<Value>> {
    let (v,): (Value,) = sqlx::query_as("SELECT loyalty FROM organizations WHERE id=$1").bind(ctx.org_id).fetch_one(&s.pool).await?;
    Ok(Json(v))
}

async fn loyalty_patch(State(s): State<AppState>, ctx: AuthContext, Json(body): Json<Value>) -> AppResult<Json<Value>> {
    ctx.require("settings.edit")?;
    let (cur,): (Value,) = sqlx::query_as("SELECT loyalty FROM organizations WHERE id=$1").bind(ctx.org_id).fetch_one(&s.pool).await?;
    let next = apply(
        cur,
        &body,
        &[
            ("cashbackEnabled", Kind::Bool),
            ("punchCardEnabled", Kind::Bool),
            ("defaultBonusLevel", Kind::Num(0.0, 100.0)),
            ("punchCardCapacity", Kind::Num(2.0, 100.0)),
        ],
    )?;
    sqlx::query("UPDATE organizations SET loyalty=$2 WHERE id=$1").bind(ctx.org_id).bind(&next).execute(&s.pool).await?;
    Ok(Json(next))
}

async fn sale_get(State(s): State<AppState>, ctx: AuthContext) -> AppResult<Json<Value>> {
    let (v,): (Value,) = sqlx::query_as("SELECT sale_settings FROM organizations WHERE id=$1").bind(ctx.org_id).fetch_one(&s.pool).await?;
    Ok(Json(v))
}

async fn sale_patch(State(s): State<AppState>, ctx: AuthContext, Json(body): Json<Value>) -> AppResult<Json<Value>> {
    ctx.require("settings.edit")?;
    let (cur,): (Value,) = sqlx::query_as("SELECT sale_settings FROM organizations WHERE id=$1").bind(ctx.org_id).fetch_one(&s.pool).await?;
    let next = apply(
        cur,
        &body,
        &[
            ("deliveryPrice", Kind::Num(0.0, 1e9)),
            ("deliveryPaidByCustomer", Kind::Bool),
            ("deliveryIncludeInReceipt", Kind::Bool),
            ("deliveryPaymentSourceWallet", Kind::OptUuid),
            ("defaultDeliverySpendingCategory", Kind::OptUuid),
            ("showSaveCartForLater", Kind::Bool),
            ("allowMultipleProductSelection", Kind::Bool),
            ("dateFieldVisible", Kind::Bool),
            ("storeFieldVisible", Kind::Bool),
            ("tableNumberFieldVisible", Kind::Bool),
            ("commentFieldVisible", Kind::Bool),
            ("mergeSimilarItems", Kind::Bool),
            ("showPurchasePriceInEditCartItem", Kind::Bool),
            ("showTotalPurchasePriceInCart", Kind::Bool),
        ],
    )?;
    sqlx::query("UPDATE organizations SET sale_settings=$2 WHERE id=$1").bind(ctx.org_id).bind(&next).execute(&s.pool).await?;
    Ok(Json(next))
}

async fn market_get(State(s): State<AppState>, ctx: AuthContext) -> AppResult<Json<Value>> {
    let (v,): (Value,) = sqlx::query_as("SELECT marketplace FROM organizations WHERE id=$1").bind(ctx.org_id).fetch_one(&s.pool).await?;
    Ok(Json(v))
}

async fn market_patch(State(s): State<AppState>, ctx: AuthContext, Json(body): Json<Value>) -> AppResult<Json<Value>> {
    ctx.require("settings.edit")?;
    let (cur,): (Value,) = sqlx::query_as("SELECT marketplace FROM organizations WHERE id=$1").bind(ctx.org_id).fetch_one(&s.pool).await?;
    let next = apply(
        cur,
        &body,
        &[
            ("marketplaceEnabled", Kind::Bool),
            ("linkEnabled", Kind::Bool),
            ("description", Kind::Text),
            ("storeForMarketplace", Kind::OptUuid),
            ("disableNotInStockProducts", Kind::Bool),
            ("enabledCategories", Kind::BoolMap),
        ],
    )?;
    if let Some(store) = next.get("storeForMarketplace").and_then(Value::as_str).and_then(|v| Uuid::parse_str(v).ok()) {
        let (n,): (i64,) = sqlx::query_as("SELECT count(*) FROM stores WHERE id=$1 AND organization_id=$2 AND deleted_at IS NULL")
            .bind(store)
            .bind(ctx.org_id)
            .fetch_one(&s.pool)
            .await?;
        if n == 0 {
            return Err(AppError::BadRequest("Филиал не найден".into()));
        }
    }
    sqlx::query("UPDATE organizations SET marketplace=$2 WHERE id=$1").bind(ctx.org_id).bind(&next).execute(&s.pool).await?;
    Ok(Json(next))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn apply_validates_keys_and_types() {
        let spec = [("a", Kind::Bool), ("n", Kind::Num(1.0, 5.0)), ("u", Kind::OptUuid)];
        let ok = apply(json!({"a": false}), &json!({"n": 3, "u": null}), &spec).unwrap();
        assert_eq!(ok, json!({"a": false, "n": 3, "u": null}));
        assert!(apply(json!({}), &json!({"n": 9}), &spec).is_err());
        assert!(apply(json!({}), &json!({"zzz": 1}), &spec).is_err());
        assert!(apply(json!({}), &json!({"u": "not-a-uuid"}), &spec).is_err());
        assert!(apply(json!({}), &json!([1]), &spec).is_err());
    }
}
