use axum::Json;
use serde_json::{json, Value};

pub async fn root() -> Json<Value> {
    Json(json!({ "status": "ok", "service": "atlas-api" }))
}

pub async fn detailed() -> Json<Value> {
    Json(json!({
        "status": "ok",
        "version": env!("CARGO_PKG_VERSION"),
        "time": chrono::Utc::now().to_rfc3339(),
    }))
}
