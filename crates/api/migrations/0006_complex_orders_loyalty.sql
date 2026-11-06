-- Лояльность, настройки продаж и маркетплейса, комплексные продажи, заказы клиентов.
ALTER TABLE organizations
    ADD COLUMN loyalty jsonb NOT NULL DEFAULT
        '{"cashbackEnabled":true,"punchCardEnabled":false,"defaultBonusLevel":1,"punchCardCapacity":7}',
    ADD COLUMN marketplace jsonb NOT NULL DEFAULT
        '{"marketplaceEnabled":false,"linkEnabled":false,"description":"","storeForMarketplace":null,"disableNotInStockProducts":false,"enabledCategories":{}}',
    ADD COLUMN sale_settings jsonb NOT NULL DEFAULT
        '{"deliveryPrice":0,"deliveryPaidByCustomer":true,"deliveryIncludeInReceipt":true,"showSaveCartForLater":true,"allowMultipleProductSelection":false,"mergeSimilarItems":true}';

ALTER TABLE clients
    ADD COLUMN address text NOT NULL DEFAULT '',
    ADD COLUMN client_type text NOT NULL DEFAULT 'person' CHECK (client_type IN ('person','organization')),
    ADD COLUMN discount_id uuid REFERENCES list_items(id),     -- постоянная скидка
    ADD COLUMN punch_punches integer NOT NULL DEFAULT 0 CHECK (punch_punches >= 0);

CREATE TABLE client_bonus_ops (
    id            uuid PRIMARY KEY,
    organization_id uuid NOT NULL REFERENCES organizations(id),
    client_id     uuid NOT NULL REFERENCES clients(id),
    operation_id  text NOT NULL,
    delta         numeric(18,2) NOT NULL,
    bonus_before  numeric(18,2) NOT NULL,
    bonus_after   numeric(18,2) NOT NULL,
    comment       text NOT NULL DEFAULT '',
    user_id       uuid REFERENCES users(id),
    created_at    timestamptz NOT NULL DEFAULT now(),
    UNIQUE (client_id, operation_id)
);

ALTER TABLE sales
    ADD COLUMN punch_card_reduction numeric(18,2) NOT NULL DEFAULT 0,
    ADD COLUMN punch_delta integer NOT NULL DEFAULT 0,
    ADD COLUMN delivery_address text NOT NULL DEFAULT '',
    ADD COLUMN delivery_phone text NOT NULL DEFAULT '',
    ADD COLUMN delivery_price numeric(18,2) NOT NULL DEFAULT 0,
    ADD COLUMN order_id uuid;

-- Новые виды проводок по денежным счетам (комплексные продажи, зарплата, касса смены, платежи).
ALTER TABLE wallet_transactions DROP CONSTRAINT wallet_transactions_kind_check;
ALTER TABLE wallet_transactions ADD CONSTRAINT wallet_transactions_kind_check CHECK (kind IN
    ('spending','supply_payment','manual','transfer','complex_sale_payment','salary','cashbox','payment','shift_close'));

-- Комплексные продажи (под заказ): расходы-материалы, стоимость для клиента, оплаты.
CREATE TABLE complex_sale_templates (
    id              uuid PRIMARY KEY,
    organization_id uuid NOT NULL REFERENCES organizations(id),
    name            text NOT NULL,
    final_price     numeric(18,2) NOT NULL DEFAULT 0,
    items           jsonb NOT NULL DEFAULT '[]',
    created_at      timestamptz NOT NULL DEFAULT now(),
    deleted_at      timestamptz
);

CREATE TABLE complex_sales (
    id              uuid PRIMARY KEY,
    organization_id uuid NOT NULL REFERENCES organizations(id),
    store_id        uuid NOT NULL REFERENCES stores(id),
    user_id         uuid NOT NULL REFERENCES users(id),
    client_id       uuid REFERENCES clients(id),
    template_name   text NOT NULL DEFAULT '',
    name            text NOT NULL,
    status          text NOT NULL DEFAULT 'new' CHECK (status IN ('new','in_progress','done','cancelled')),
    comment         text NOT NULL DEFAULT '',
    final_price     numeric(18,2) NOT NULL DEFAULT 0,
    sale_date       timestamptz NOT NULL DEFAULT now(),
    created_at      timestamptz NOT NULL DEFAULT now(),
    deleted_at      timestamptz
);
CREATE INDEX complex_sales_org_idx ON complex_sales (organization_id, store_id, sale_date) WHERE deleted_at IS NULL;

CREATE TABLE complex_sale_items (
    id          uuid PRIMARY KEY,
    sale_id     uuid NOT NULL REFERENCES complex_sales(id) ON DELETE CASCADE,
    product_id  uuid REFERENCES products(id),      -- NULL = пустая строка (ручной расход)
    name        text NOT NULL,
    qty         numeric(18,4) NOT NULL CHECK (qty > 0),
    cost        numeric(18,4) NOT NULL DEFAULT 0 CHECK (cost >= 0),
    extra_cost  numeric(18,4) NOT NULL DEFAULT 0 CHECK (extra_cost >= 0)
);

CREATE TABLE complex_sale_payments (
    id          uuid PRIMARY KEY,
    sale_id     uuid NOT NULL REFERENCES complex_sales(id) ON DELETE CASCADE,
    wallet_id   uuid REFERENCES wallets(id),
    amount      numeric(18,2) NOT NULL CHECK (amount > 0),
    comment     text NOT NULL DEFAULT '',
    user_id     uuid REFERENCES users(id),
    paid_at     timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE complex_sale_updates (
    id          uuid PRIMARY KEY,
    sale_id     uuid NOT NULL REFERENCES complex_sales(id) ON DELETE CASCADE,
    user_id     uuid REFERENCES users(id),
    kind        text NOT NULL,                      -- status | comment
    text        text NOT NULL,
    created_at  timestamptz NOT NULL DEFAULT clock_timestamp()
);

-- Заказы клиентов (онлайн-каталог и ручные).
CREATE TABLE client_orders (
    id              uuid PRIMARY KEY,
    organization_id uuid NOT NULL REFERENCES organizations(id),
    store_id        uuid NOT NULL REFERENCES stores(id),
    number          bigint GENERATED ALWAYS AS IDENTITY,
    source          text NOT NULL DEFAULT 'manual' CHECK (source IN ('manual','marketplace')),
    client_id       uuid REFERENCES clients(id),
    client_name     text NOT NULL DEFAULT '',
    client_phone    text NOT NULL DEFAULT '',
    client_email    text NOT NULL DEFAULT '',
    items           jsonb NOT NULL,
    total           numeric(18,2) NOT NULL,
    status          text NOT NULL DEFAULT 'new' CHECK (status IN ('new','in_work','ready','done','cancelled')),
    payment_method  text NOT NULL DEFAULT '',
    delivery_address text NOT NULL DEFAULT '',
    notes           text NOT NULL DEFAULT '',
    sale_id         uuid REFERENCES sales(id),
    created_at      timestamptz NOT NULL DEFAULT now(),
    updated_at      timestamptz NOT NULL DEFAULT now(),
    deleted_at      timestamptz
);
CREATE INDEX client_orders_org_idx ON client_orders (organization_id, store_id, created_at) WHERE deleted_at IS NULL;
