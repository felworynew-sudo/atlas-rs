//! Журнал движений остатков: каждое изменение остатка — запись в `product_movements` + обновление `store_stock`.
use crate::error::AppResult;
use atlas_domain::sale::StockNode;
use sqlx::{Postgres, Transaction};
use std::collections::HashMap;
use uuid::Uuid;

fn round4(x: f64) -> f64 {
    (x * 10_000.0).round() / 10_000.0
}

/// Применить движение; возвращает остаток после него.
#[allow(clippy::too_many_arguments)]
pub async fn apply(
    tx: &mut Transaction<'_, Postgres>,
    org_id: Uuid,
    store_id: Uuid,
    product_id: Uuid,
    kind: &str,
    delta: f64,
    ref_id: Option<Uuid>,
    user_id: Uuid,
) -> AppResult<f64> {
    let (after,): (f64,) = sqlx::query_as(
        "INSERT INTO store_stock (product_id, store_id, qty) VALUES ($1,$2,$3::float8) \
         ON CONFLICT (product_id, store_id) DO UPDATE SET qty = store_stock.qty + $3::float8 \
         RETURNING qty::float8",
    )
    .bind(product_id)
    .bind(store_id)
    .bind(delta)
    .fetch_one(&mut **tx)
    .await?;
    let after = round4(after);
    sqlx::query(
        "INSERT INTO product_movements (id, organization_id, product_id, store_id, kind, delta, qty_before, ref_id, user_id) \
         VALUES ($1,$2,$3,$4,$5,$6::float8,$7::float8,$8,$9)",
    )
    .bind(Uuid::new_v4())
    .bind(org_id)
    .bind(product_id)
    .bind(store_id)
    .bind(kind)
    .bind(delta)
    .bind(round4(after - delta))
    .bind(ref_id)
    .bind(user_id)
    .execute(&mut **tx)
    .await?;
    Ok(after)
}

/// Граф каталога для расчёта списаний.
pub async fn stock_nodes(pool: &sqlx::PgPool, org_id: Uuid) -> AppResult<HashMap<Uuid, StockNode<Uuid>>> {
    let prods: Vec<(Uuid, bool, bool, String)> = sqlx::query_as(
        "SELECT id, is_service, is_composite, production_type FROM products WHERE organization_id=$1 AND deleted_at IS NULL",
    )
    .bind(org_id)
    .fetch_all(pool)
    .await?;
    let edges: Vec<(Uuid, Uuid, f64)> = sqlx::query_as(
        "SELECT i.product_id, i.ingredient_id, i.qty::float8 FROM product_ingredients i \
         JOIN products p ON p.id = i.product_id WHERE p.organization_id=$1 AND p.deleted_at IS NULL",
    )
    .bind(org_id)
    .fetch_all(pool)
    .await?;
    let mut map: HashMap<Uuid, StockNode<Uuid>> = prods
        .into_iter()
        .map(|(id, service, composite, production)| {
            (id, StockNode { service, on_sale_composite: composite && production == "on_sale", ingredients: vec![] })
        })
        .collect();
    for (p, i, q) in edges {
        if let Some(n) = map.get_mut(&p) {
            n.ingredients.push((i, q));
        }
    }
    Ok(map)
}
