# Atlas (Rust)

Воссоздание CRM «Атлас» по каталогу `D:\Atlas-catalog` (ТЗ). Стек: axum 0.8, sqlx 0.9 (PostgreSQL), argon2, JWT.

## Состояние
Сделано (шаг 1 по порядку из `rust-implementation.md`):
- `crates/domain` — модули организации (17), 51 право, роли «Админ»/«Продавец»; юнит-тесты.
- `crates/api` — миграция `0001_core.sql`; вход/регистрация, организации (модули), филиалы, роли, сотрудники, справочник прав; единый формат ошибок `{statusCode, message, error}`.
- `tests/core_flow.rs` — сквозной тест на реальном PostgreSQL.

Шаг 2 — каталог: `0002_catalog.sql`, `/api/product-categories`, `/api/suppliers`, `/api/product-units` (5 единиц по умолчанию при регистрации), `/api/products` (цены и доступность по филиалам, штрихкоды, составные товары с расчётом себестоимости и защитой от циклов, копия, поиск/фильтры); `domain::catalog`; тест `catalog_flow`.

Шаг 3 — продажа: `0003_sales.sql`; `/api/lists/{kind}` (методы оплаты/покупки, скидки, причины, категории расходов — значения по умолчанию при регистрации), `/api/clients`, `/api/saved-carts`, `/api/sales` (скидка, надбавка, бонусы и кэшбэк, смешанная оплата, кастомные позиции, идемпотентность, списание остатков с учётом составных «во время продажи», возвраты, удаление со сторно), `/api/stocks` (остатки, история движений, ручная установка); `domain::sale`; тест `sale_flow`.

Дальше: приёмка/списание/ревизия, остатки и журнал движений, приёмка/списание/ревизия, расходы, отчёты.

## Запуск
```bash
export DATABASE_URL=postgres://postgres:secret@127.0.0.1:54329/atlas
export JWT_SECRET=change-me
cargo run -p atlas-api        # слушает 0.0.0.0:8080, миграции применяются при старте
```

## Тесты
```bash
cargo test -p atlas-domain
TEST_DATABASE_URL=postgres://postgres:secret@127.0.0.1:54329/atlas_test cargo test -p atlas-api
```
Без `TEST_DATABASE_URL` тест пытается скачать встроенный PostgreSQL с GitHub API (может упереться в rate-limit).

Тесты с `TEST_DATABASE_URL` пересоздают схему `public` — используйте отдельную тестовую БД.

## Заметки по окружению
- Диск D почти полон: сборку веду на C (`CARGO_TARGET_DIR=C:/Users/shika/atlas-target`), в `Cargo.toml` `debug = 0`.
- Локальный PostgreSQL 18.6 лежит в `C:\Users\shika\atlas-pg` (порт 54329, пароль `secret`, только для разработки).
