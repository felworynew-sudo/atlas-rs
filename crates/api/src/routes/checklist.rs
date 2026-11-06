//! /api/checklist — чек-лист дня: пункты филиала, отметки выполнения (сбрасываются каждый день), редактирование
use crate::{
    auth::AuthContext,
    error::{AppError, AppResult},
    routes::sales::check_store,
    AppState,
};
use axum::{
    extract::{Path, Query, State},
    routing::{get, post, put},
    Json, Router,
};
use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

pub fn router() -> Router<AppState> {
    Router::new().route("/", get(list)).route("/items", put(replace_items)).route("/items/{id}/toggle", post(toggle))
}

#[derive(Serialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct ItemView {
    pub id: Uuid,
    pub title: String,
    pub time: String,
    pub sort_order: i32,
    pub done: bool,
    pub done_at: Option<DateTime<Utc>>,
    pub done_by: Option<String>,
}

const SELECT_ITEMS: &str = "SELECT i.id, i.title, i.time_of_day AS time, i.sort_order, (m.done_at IS NOT NULL) AS done, m.done_at, u.name AS done_by \
     FROM checklist_items i JOIN stores st ON st.id = i.store_id \
     LEFT JOIN checklist_marks m ON m.item_id = i.id AND m.mark_date = COALESCE($3, (now() AT TIME ZONE st.timezone)::date) \
     LEFT JOIN users u ON u.id = m.user_id \
     WHERE i.organization_id=$1 AND i.store_id=$2 AND i.deleted_at IS NULL AND ($4::uuid IS NULL OR i.id=$4) \
     ORDER BY i.sort_order, i.time_of_day, i.created_at";

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListQuery {
    pub store_id: Uuid,
    pub date: Option<NaiveDate>,
}

async fn list(State(s): State<AppState>, ctx: AuthContext, Query(q): Query<ListQuery>) -> AppResult<Json<Vec<ItemView>>> {
    ctx.require("checklist.view")?;
    check_store(&s.pool, &ctx, q.store_id).await?;
    Ok(Json(
        sqlx::query_as::<_, ItemView>(SELECT_ITEMS).bind(ctx.org_id).bind(q.store_id).bind(q.date).bind(None::<Uuid>).fetch_all(&s.pool).await?,
    ))
}

fn time_ok(t: &str) -> bool {
    t.is_empty()
        || (t.len() == 5 && t.as_bytes()[2] == b':' && t[..2].parse::<u8>().is_ok_and(|h| h < 24) && t[3..].parse::<u8>().is_ok_and(|m| m < 60))
}

#[derive(Deserialize)]
pub struct ItemDto {
    pub id: Option<Uuid>,
    pub title: String,
    #[serde(default)]
    pub time: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplaceDto {
    pub store_id: Uuid,
    pub items: Vec<ItemDto>,
}

/// Режим редактирования: список пунктов заменяется целиком (порядок = порядок в массиве).
async fn replace_items(State(s): State<AppState>, ctx: AuthContext, Json(dto): Json<ReplaceDto>) -> AppResult<Json<Vec<ItemView>>> {
    ctx.require("checklist.edit")?;
    check_store(&s.pool, &ctx, dto.store_id).await?;
    for it in &dto.items {
        if it.title.trim().is_empty() {
            return Err(AppError::BadRequest("Введите название пункта".into()));
        }
        if !time_ok(&it.time) {
            return Err(AppError::BadRequest("Время в формате ЧЧ:ММ".into()));
        }
    }
    let mut tx = s.pool.begin().await?;
    let keep: Vec<Uuid> = dto.items.iter().filter_map(|i| i.id).collect();
    sqlx::query("UPDATE checklist_items SET deleted_at=now() WHERE organization_id=$1 AND store_id=$2 AND deleted_at IS NULL AND NOT (id = ANY($3))")
        .bind(ctx.org_id)
        .bind(dto.store_id)
        .bind(&keep)
        .execute(&mut *tx)
        .await?;
    for (n, it) in dto.items.iter().enumerate() {
        match it.id {
            Some(id) => {
                let res = sqlx::query(
                    "UPDATE checklist_items SET title=$4, time_of_day=$5, sort_order=$6 WHERE id=$1 AND organization_id=$2 AND store_id=$3 AND deleted_at IS NULL",
                )
                .bind(id)
                .bind(ctx.org_id)
                .bind(dto.store_id)
                .bind(it.title.trim())
                .bind(&it.time)
                .bind(n as i32)
                .execute(&mut *tx)
                .await?;
                if res.rows_affected() == 0 {
                    return Err(AppError::NotFound("Пункт не найден".into()));
                }
            }
            None => {
                sqlx::query("INSERT INTO checklist_items (id, organization_id, store_id, title, time_of_day, sort_order) VALUES ($1,$2,$3,$4,$5,$6)")
                    .bind(Uuid::new_v4())
                    .bind(ctx.org_id)
                    .bind(dto.store_id)
                    .bind(it.title.trim())
                    .bind(&it.time)
                    .bind(n as i32)
                    .execute(&mut *tx)
                    .await?;
            }
        }
    }
    tx.commit().await?;
    Ok(Json(
        sqlx::query_as::<_, ItemView>(SELECT_ITEMS).bind(ctx.org_id).bind(dto.store_id).bind(None::<NaiveDate>).bind(None::<Uuid>).fetch_all(&s.pool).await?,
    ))
}

#[derive(Deserialize, Default)]
pub struct ToggleDto {
    pub date: Option<NaiveDate>,
}

/// Отметка пункта: первый вызов записывает время и сотрудника, второй снимает отметку.
async fn toggle(
    State(s): State<AppState>,
    ctx: AuthContext,
    Path(id): Path<Uuid>,
    body: Option<Json<ToggleDto>>,
) -> AppResult<Json<ItemView>> {
    ctx.require("checklist.view")?;
    let date = body.and_then(|b| b.0.date);
    let row: Option<(Uuid,)> = sqlx::query_as("SELECT store_id FROM checklist_items WHERE id=$1 AND organization_id=$2 AND deleted_at IS NULL")
        .bind(id)
        .bind(ctx.org_id)
        .fetch_optional(&s.pool)
        .await?;
    let (store_id,) = row.ok_or_else(|| AppError::NotFound("Пункт не найден".into()))?;
    check_store(&s.pool, &ctx, store_id).await?;
    let res = sqlx::query(
        "DELETE FROM checklist_marks m USING checklist_items i JOIN stores st ON st.id = i.store_id \
         WHERE m.item_id = i.id AND i.id=$1 AND m.mark_date = COALESCE($2, (now() AT TIME ZONE st.timezone)::date)",
    )
    .bind(id)
    .bind(date)
    .execute(&s.pool)
    .await?;
    if res.rows_affected() == 0 {
        sqlx::query(
            "INSERT INTO checklist_marks (item_id, mark_date, user_id) \
             SELECT i.id, COALESCE($2, (now() AT TIME ZONE st.timezone)::date), $3 FROM checklist_items i JOIN stores st ON st.id = i.store_id WHERE i.id=$1",
        )
        .bind(id)
        .bind(date)
        .bind(ctx.user_id)
        .execute(&s.pool)
        .await?;
    }
    sqlx::query_as::<_, ItemView>(SELECT_ITEMS)
        .bind(ctx.org_id)
        .bind(store_id)
        .bind(date)
        .bind(Some(id))
        .fetch_optional(&s.pool)
        .await?
        .map(Json)
        .ok_or_else(|| AppError::NotFound("Пункт не найден".into()))
}
