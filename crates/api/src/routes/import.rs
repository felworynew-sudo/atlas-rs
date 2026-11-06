//! Импорт и экспорт: /api/import/{products,sales,batches/{id}}, /api/export/{products,clients}.
//! Форматы файлов — CSV и TSV (разделитель определяется автоматически), содержимое передаётся строкой `content`.
use crate::{
    auth::AuthContext,
    error::{AppError, AppResult},
    ledger,
    routes::{
        reports::csv_response,
        sales::{self, check_store, CreateSaleDto, ItemDto},
    },
    AppState,
};
use atlas_domain::csv::{detect_delimiter, parse, parse_number};
use axum::{
    extract::{Path, Query, State},
    response::{IntoResponse, Response},
    routing::{delete, get, post},
    Json, Router,
};
use chrono::{DateTime, NaiveDate, NaiveDateTime, TimeZone, Utc};
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::{Acquire, Postgres, Transaction};
use std::collections::HashMap;
use uuid::Uuid;

pub fn import_router() -> Router<AppState> {
    Router::new()
        .route("/products", post(import_products))
        .route("/sales", post(import_sales))
        .route("/batches/{id}", delete(revert_batch))
}

pub fn export_router() -> Router<AppState> {
    Router::new().route("/products", get(export_products)).route("/clients", get(export_clients))
}

// ---------- таблица ----------
struct Table {
    head: Vec<String>,
    rows: Vec<(usize, Vec<String>)>,
}

impl Table {
    fn read(content: &str) -> AppResult<Table> {
        let mut rows = parse(content, detect_delimiter(content)).map_err(AppError::BadRequest)?;
        if rows.is_empty() {
            return Err(AppError::BadRequest("Файл пустой".into()));
        }
        let (_, head) = rows.remove(0);
        Ok(Table { head: head.into_iter().map(|h| h.trim().to_lowercase()).collect(), rows })
    }

    fn col(&self, names: &[&str]) -> Option<usize> {
        names.iter().find_map(|n| self.head.iter().position(|h| h == n))
    }
}

fn cell(row: &[String], i: Option<usize>) -> &str {
    i.and_then(|i| row.get(i)).map_or("", |s| s.trim())
}

fn number(row: &[String], i: Option<usize>, what: &str) -> AppResult<Option<f64>> {
    let raw = cell(row, i);
    if raw.is_empty() {
        return Ok(None);
    }
    match parse_number(raw) {
        Some(v) if v >= 0.0 => Ok(Some(v)),
        _ => Err(AppError::BadRequest(format!("Некорректное значение «{what}»: {raw}"))),
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportDto {
    pub store_id: Uuid,
    pub content: String,
    #[serde(default)]
    pub dry_run: bool,
}

// =========================== товары ===========================
struct ProductCols {
    name: Option<usize>,
    price: Option<usize>,
    purchase: Option<usize>,
    barcode: Option<usize>,
    category: Option<usize>,
    supplier: Option<usize>,
    stock: Option<usize>,
}

async fn named_ref(tx: &mut Transaction<'_, Postgres>, org: Uuid, table: u8, name: &str) -> AppResult<Option<Uuid>> {
    if name.is_empty() {
        return Ok(None);
    }
    // 0 — категории, 1 — поставщики
    let found: Option<(Uuid,)> = if table == 0 {
        sqlx::query_as("SELECT id FROM product_categories WHERE organization_id=$1 AND deleted_at IS NULL AND lower(name)=lower($2) LIMIT 1")
            .bind(org)
            .bind(name)
            .fetch_optional(&mut **tx)
            .await?
    } else {
        sqlx::query_as("SELECT id FROM suppliers WHERE organization_id=$1 AND deleted_at IS NULL AND lower(name)=lower($2) LIMIT 1")
            .bind(org)
            .bind(name)
            .fetch_optional(&mut **tx)
            .await?
    };
    if let Some((id,)) = found {
        return Ok(Some(id));
    }
    let id = Uuid::new_v4();
    if table == 0 {
        sqlx::query(
            "INSERT INTO product_categories (id, organization_id, name, sort_order) VALUES ($1,$2,$3,\
             (SELECT COALESCE(max(sort_order),0)+1 FROM product_categories WHERE organization_id=$2))",
        )
        .bind(id)
        .bind(org)
        .bind(name)
        .execute(&mut **tx)
        .await?;
    } else {
        sqlx::query("INSERT INTO suppliers (id, organization_id, name) VALUES ($1,$2,$3)").bind(id).bind(org).bind(name).execute(&mut **tx).await?;
    }
    Ok(Some(id))
}

/// Одна строка файла товаров; возвращает "created" или "updated".
async fn product_row(
    tx: &mut Transaction<'_, Postgres>,
    ctx: &AuthContext,
    store: Uuid,
    c: &ProductCols,
    row: &[String],
) -> AppResult<&'static str> {
    let org = ctx.org_id;
    let name = cell(row, c.name);
    if name.is_empty() {
        return Err(AppError::BadRequest("Не указано название".into()));
    }
    let price = number(row, c.price, "цена")?;
    let purchase = number(row, c.purchase, "закупочная цена")?;
    let stock = number(row, c.stock, "остаток")?;
    let barcode = cell(row, c.barcode);
    let category = named_ref(tx, org, 0, cell(row, c.category)).await?;
    let supplier = named_ref(tx, org, 1, cell(row, c.supplier)).await?;

    // товар ищется по штрихкоду, иначе по названию
    let mut found: Option<(Uuid, bool, bool)> = None;
    if !barcode.is_empty() {
        found = sqlx::query_as(
            "SELECT id, is_service, allow_fractional FROM products WHERE organization_id=$1 AND deleted_at IS NULL AND $2 = ANY(barcodes) LIMIT 1",
        )
        .bind(org)
        .bind(barcode)
        .fetch_optional(&mut **tx)
        .await?;
    }
    if found.is_none() {
        found = sqlx::query_as(
            "SELECT id, is_service, allow_fractional FROM products WHERE organization_id=$1 AND deleted_at IS NULL AND lower(name)=lower($2) LIMIT 1",
        )
        .bind(org)
        .bind(name)
        .fetch_optional(&mut **tx)
        .await?;
    }
    let (id, kind, service, fractional) = match found {
        Some((id, service, fractional)) => {
            sqlx::query(
                "UPDATE products SET purchase_price=COALESCE($3::float8, purchase_price), category_id=COALESCE($4, category_id), \
                 supplier_id=COALESCE($5, supplier_id), \
                 barcodes = CASE WHEN $6 = '' OR $6 = ANY(barcodes) THEN barcodes ELSE barcodes || $6::text END \
                 WHERE id=$1 AND organization_id=$2",
            )
            .bind(id)
            .bind(org)
            .bind(purchase)
            .bind(category)
            .bind(supplier)
            .bind(barcode)
            .execute(&mut **tx)
            .await?;
            (id, "updated", service, fractional)
        }
        None => {
            let id = Uuid::new_v4();
            let barcodes: Vec<String> = if barcode.is_empty() { vec![] } else { vec![barcode.to_string()] };
            sqlx::query(
                "INSERT INTO products (id, organization_id, name, purchase_price, category_id, supplier_id, barcodes, unit_id) \
                 VALUES ($1,$2,$3,COALESCE($4::float8,0),$5,$6,$7,\
                 (SELECT id FROM product_units WHERE organization_id=$2 AND deleted_at IS NULL ORDER BY created_at LIMIT 1))",
            )
            .bind(id)
            .bind(org)
            .bind(name)
            .bind(purchase)
            .bind(category)
            .bind(supplier)
            .bind(&barcodes)
            .execute(&mut **tx)
            .await?;
            (id, "created", false, false)
        }
    };
    if let Some(p) = price {
        sqlx::query(
            "INSERT INTO product_stores (product_id, store_id, active, sale_price) VALUES ($1,$2,true,$3::float8) \
             ON CONFLICT (product_id, store_id) DO UPDATE SET sale_price = EXCLUDED.sale_price",
        )
        .bind(id)
        .bind(store)
        .bind(p)
        .execute(&mut **tx)
        .await?;
    }
    sqlx::query(
        "INSERT INTO product_stores (product_id, store_id, active, sale_price) \
         SELECT $1, st.id, true, 0 FROM stores st WHERE st.organization_id=$2 AND st.deleted_at IS NULL ON CONFLICT DO NOTHING",
    )
    .bind(id)
    .bind(org)
    .execute(&mut **tx)
    .await?;
    if let Some(q) = stock {
        if !service {
            if !fractional && q.fract() != 0.0 {
                return Err(AppError::BadRequest(format!("Остаток «{name}» должен быть целым")));
            }
            let cur = ledger::lock_qty(tx, store, id).await?;
            let delta = ((q - cur) * 10_000.0).round() / 10_000.0;
            if delta != 0.0 {
                ledger::apply(tx, org, store, id, "manual", delta, None, ctx.user_id).await?;
            }
        }
    }
    Ok(kind)
}

async fn import_products(State(s): State<AppState>, ctx: AuthContext, Json(dto): Json<ImportDto>) -> AppResult<Json<Value>> {
    ctx.require("settings.export_import")?;
    check_store(&s.pool, &ctx, dto.store_id).await?;
    let table = Table::read(&dto.content)?;
    let cols = ProductCols {
        name: table.col(&["название", "name"]),
        price: table.col(&["цена", "цена продажи", "price"]),
        purchase: table.col(&["закупочная цена", "purchaseprice", "purchase price"]),
        barcode: table.col(&["штрихкод", "barcode"]),
        category: table.col(&["категория", "category"]),
        supplier: table.col(&["поставщик", "supplier"]),
        stock: table.col(&["остаток", "stock"]),
    };
    if cols.name.is_none() {
        return Err(AppError::BadRequest("В файле нет колонки «название»".into()));
    }
    let (mut created, mut updated) = (0, 0);
    let mut errors: Vec<Value> = Vec::new();
    let mut tx = s.pool.begin().await?;
    for (line, row) in &table.rows {
        let mut sp = tx.begin().await?;
        match product_row(&mut sp, &ctx, dto.store_id, &cols, row).await {
            Ok(kind) => {
                sp.commit().await?;
                if kind == "created" {
                    created += 1;
                } else {
                    updated += 1;
                }
            }
            Err(e) => {
                sp.rollback().await?;
                errors.push(json!({ "row": line, "message": format!("Строка {line}: {e}") }));
            }
        }
    }
    if dto.dry_run {
        tx.rollback().await?;
    } else {
        tx.commit().await?;
    }
    Ok(Json(json!({ "dryRun": dto.dry_run, "created": created, "updated": updated, "errors": errors })))
}

// =========================== продажи ===========================
fn parse_date(raw: &str) -> Option<DateTime<Utc>> {
    let raw = raw.trim();
    for f in ["%Y-%m-%d %H:%M", "%d.%m.%Y %H:%M", "%d/%m/%Y %H:%M"] {
        if let Ok(dt) = NaiveDateTime::parse_from_str(raw, f) {
            return Some(Utc.from_utc_datetime(&dt));
        }
    }
    for f in ["%Y-%m-%d", "%d.%m.%Y", "%d/%m/%Y"] {
        if let Ok(d) = NaiveDate::parse_from_str(raw, f) {
            return Some(Utc.from_utc_datetime(&d.and_hms_opt(12, 0, 0)?));
        }
    }
    None
}

struct SaleGroup {
    line: usize,
    phone: String,
    date: DateTime<Utc>,
    items: Vec<(Uuid, f64, Option<f64>)>,
}

async fn import_sales(State(s): State<AppState>, ctx: AuthContext, Json(dto): Json<ImportDto>) -> AppResult<Json<Value>> {
    ctx.require("settings.export_import")?;
    ctx.require("sale.new")?;
    check_store(&s.pool, &ctx, dto.store_id).await?;
    let table = Table::read(&dto.content)?;
    let c_sku = table.col(&["артикул", "sku", "штрихкод", "barcode", "название", "name"]);
    let c_qty = table.col(&["количество", "qty", "quantity"]);
    let c_client = table.col(&["клиент", "телефон", "phone", "client"]);
    let c_date = table.col(&["дата", "date"]);
    let c_price = table.col(&["цена", "price"]);
    if c_sku.is_none() || c_qty.is_none() {
        return Err(AppError::BadRequest("В файле нужны колонки «артикул» и «количество»".into()));
    }

    // каталог для разбора артикулов: штрихкод, название или идентификатор → (товар, цена в филиале)
    let prods: Vec<(Uuid, String, Vec<String>, f64)> = sqlx::query_as(
        "SELECT p.id, p.name, p.barcodes, COALESCE(ps.sale_price,0)::float8 FROM products p \
         LEFT JOIN product_stores ps ON ps.product_id=p.id AND ps.store_id=$2 WHERE p.organization_id=$1 AND p.deleted_at IS NULL",
    )
    .bind(ctx.org_id)
    .bind(dto.store_id)
    .fetch_all(&s.pool)
    .await?;
    let mut by_key: HashMap<String, (Uuid, f64)> = HashMap::new();
    for (id, name, barcodes, price) in &prods {
        by_key.insert(name.to_lowercase(), (*id, *price));
        by_key.insert(id.to_string(), (*id, *price));
        for b in barcodes {
            by_key.insert(b.to_lowercase(), (*id, *price));
        }
    }

    let mut errors: Vec<Value> = Vec::new();
    let mut groups: Vec<SaleGroup> = Vec::new();
    let mut index: HashMap<(String, String), usize> = HashMap::new();
    let (mut items_total, mut total) = (0usize, 0.0f64);
    let today = Utc::now().format("%Y-%m-%d").to_string();
    for (line, row) in &table.rows {
        let mut fail = |m: String| errors.push(json!({ "row": line, "message": format!("Строка {line}: {m}") }));
        let sku = cell(row, c_sku);
        let Some((pid, store_price)) = by_key.get(&sku.to_lowercase()).copied() else {
            fail(format!("не найден артикул «{sku}»"));
            continue;
        };
        let qty = match parse_number(cell(row, c_qty)) {
            Some(q) if q > 0.0 => q,
            _ => {
                fail("количество должно быть больше нуля".into());
                continue;
            }
        };
        let phone = cell(row, c_client).to_string();
        if !phone.is_empty() && !phone.starts_with('+') {
            fail("Номер телефона должен начинаться на +".into());
            continue;
        }
        let date_raw = cell(row, c_date);
        let date = if date_raw.is_empty() { parse_date(&today) } else { parse_date(date_raw) };
        let Some(date) = date else {
            fail(format!("некорректная дата «{date_raw}»"));
            continue;
        };
        let price = match number(row, c_price, "цена") {
            Ok(p) => p,
            Err(e) => {
                fail(e.to_string());
                continue;
            }
        };
        // строки с одним клиентом и датой объединяются в одну продажу
        let key = (phone.clone(), date.format("%Y-%m-%d").to_string());
        let gi = *index.entry(key).or_insert_with(|| {
            groups.push(SaleGroup { line: *line, phone, date, items: vec![] });
            groups.len() - 1
        });
        groups[gi].items.push((pid, qty, price));
        items_total += 1;
        total += qty * price.unwrap_or(store_price);
    }

    let batch_id = Uuid::new_v4();
    let mut created = 0;
    if !dto.dry_run {
        for (n, g) in groups.iter().enumerate() {
            let client_id = if g.phone.is_empty() {
                None
            } else {
                let found: Option<(Uuid,)> =
                    sqlx::query_as("SELECT id FROM clients WHERE organization_id=$1 AND phone=$2 AND deleted_at IS NULL")
                        .bind(ctx.org_id)
                        .bind(&g.phone)
                        .fetch_optional(&s.pool)
                        .await?;
                match found {
                    Some((id,)) => Some(id),
                    None => {
                        let id = Uuid::new_v4();
                        sqlx::query("INSERT INTO clients (id, organization_id, name, phone) VALUES ($1,$2,$3,$3)")
                            .bind(id)
                            .bind(ctx.org_id)
                            .bind(&g.phone)
                            .execute(&s.pool)
                            .await?;
                        Some(id)
                    }
                }
            };
            let sale = CreateSaleDto {
                store_id: dto.store_id,
                client_id,
                items: g.items.iter().map(|(p, q, pr)| ItemDto { product_id: Some(*p), qty: *q, price: *pr, ..Default::default() }).collect(),
                sale_date: Some(g.date),
                idempotency_key: Some(format!("import-{batch_id}-{n}")),
                cashback_enabled: Some(false),
                comment: Some("Импорт".into()),
                import_batch_id: Some(batch_id),
                ..Default::default()
            };
            match sales::create(State(s.clone()), ctx.clone(), Json(sale)).await {
                Ok(_) => created += 1,
                Err(e) => errors.push(json!({ "row": g.line, "message": format!("Строка {}: {e}", g.line) })),
            }
        }
    }
    Ok(Json(json!({
        "dryRun": dto.dry_run,
        "batchId": if dto.dry_run { Value::Null } else { json!(batch_id) },
        "sales": if dto.dry_run { groups.len() } else { created },
        "items": items_total,
        "total": (total * 100.0).round() / 100.0,
        "errors": errors,
    })))
}

/// Откат партии импорта: все её продажи удаляются (остатки возвращаются).
async fn revert_batch(State(s): State<AppState>, ctx: AuthContext, Path(id): Path<Uuid>) -> AppResult<Json<Value>> {
    ctx.require("settings.export_import")?;
    ctx.require("sale.delete")?;
    let ids: Vec<(Uuid,)> =
        sqlx::query_as("SELECT id FROM sales WHERE organization_id=$1 AND import_batch_id=$2 AND deleted_at IS NULL")
            .bind(ctx.org_id)
            .bind(id)
            .fetch_all(&s.pool)
            .await?;
    if ids.is_empty() {
        return Err(AppError::NotFound("Партия импорта не найдена или уже откатена".into()));
    }
    for (sale_id,) in &ids {
        let _ = sales::remove(State(s.clone()), ctx.clone(), Path(*sale_id)).await?;
    }
    Ok(Json(json!({ "reverted": ids.len() })))
}

// =========================== экспорт ===========================
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportQuery {
    pub store_id: Option<Uuid>,
    pub format: Option<String>,
}

fn n2(v: f64) -> String {
    format!("{v:.2}")
}

async fn export_products(State(s): State<AppState>, ctx: AuthContext, Query(q): Query<ExportQuery>) -> AppResult<Response> {
    ctx.require("settings.export_import")?;
    let store = match q.store_id {
        Some(id) => {
            check_store(&s.pool, &ctx, id).await?;
            id
        }
        None => {
            let r: (Uuid,) = sqlx::query_as("SELECT id FROM stores WHERE organization_id=$1 AND deleted_at IS NULL ORDER BY created_at LIMIT 1")
                .bind(ctx.org_id)
                .fetch_one(&s.pool)
                .await?;
            r.0
        }
    };
    let rows: Vec<(String, f64, f64, Vec<String>, Option<String>, Option<String>, f64)> = sqlx::query_as(
        "SELECT p.name, COALESCE(ps.sale_price,0)::float8, p.purchase_price::float8, p.barcodes, c.name, sp.name, COALESCE(ss.qty,0)::float8 \
         FROM products p LEFT JOIN product_stores ps ON ps.product_id=p.id AND ps.store_id=$2 \
         LEFT JOIN product_categories c ON c.id=p.category_id LEFT JOIN suppliers sp ON sp.id=p.supplier_id \
         LEFT JOIN store_stock ss ON ss.product_id=p.id AND ss.store_id=$2 \
         WHERE p.organization_id=$1 AND p.deleted_at IS NULL ORDER BY p.name",
    )
    .bind(ctx.org_id)
    .bind(store)
    .fetch_all(&s.pool)
    .await?;
    if q.format.as_deref() == Some("json") {
        return Ok(Json(
            rows.iter()
                .map(|r| json!({"name": r.0, "price": r.1, "purchasePrice": r.2, "barcode": r.3.first(), "category": r.4, "supplier": r.5, "stock": r.6}))
                .collect::<Vec<_>>(),
        )
        .into_response());
    }
    let mut out = vec![["название", "цена", "закупочная цена", "штрихкод", "категория", "поставщик", "остаток"].map(String::from).to_vec()];
    for r in rows {
        out.push(vec![r.0, n2(r.1), n2(r.2), r.3.first().cloned().unwrap_or_default(), r.4.unwrap_or_default(), r.5.unwrap_or_default(), n2(r.6)]);
    }
    Ok(csv_response("products", out))
}

async fn export_clients(State(s): State<AppState>, ctx: AuthContext) -> AppResult<Response> {
    ctx.require("settings.export_import")?;
    ctx.require("client.card")?;
    let rows: Vec<(String, String, String, f64, i64, f64)> = sqlx::query_as(
        "SELECT c.name, c.phone, c.address, c.bonus_balance::float8, \
                (SELECT count(*) FROM sales s WHERE s.client_id=c.id AND s.deleted_at IS NULL), \
                (SELECT COALESCE(SUM(s.total - s.returned_amount),0) FROM sales s WHERE s.client_id=c.id AND s.deleted_at IS NULL)::float8 \
         FROM clients c WHERE c.organization_id=$1 AND c.deleted_at IS NULL ORDER BY c.name",
    )
    .bind(ctx.org_id)
    .fetch_all(&s.pool)
    .await?;
    let mut out = vec![["имя", "телефон", "адрес", "бонусы", "покупок", "сумма"].map(String::from).to_vec()];
    for r in rows {
        out.push(vec![r.0, r.1, r.2, n2(r.3), r.4.to_string(), n2(r.5)]);
    }
    Ok(csv_response("clients", out))
}
