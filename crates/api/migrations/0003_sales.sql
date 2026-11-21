-- Продажа: настраиваемые списки, клиенты, журнал остатков, продажи, возвраты, отложенные чеки.

-- Универсальные настраиваемые списки (методы оплаты/покупки, скидки, причины, категории расходов).
CREATE TABLE list_items (
    id              uuid PRIMARY KEY,
    organization_id uuid    NOT NULL REFERENCES organizations(id),
    kind            text    NOT NULL CHECK (kind IN
        ('payment_methods','purchase_methods','discounts','return_reasons','writeoff_reasons','spending_categories')),
    name            text    NOT NULL,
    params          jsonb   NOT NULL DEFAULT '{}',
    sort_order      integer NOT NULL DEFAULT 0,
    created_at      timestamptz NOT NULL DEFAULT now(),
    deleted_at      timestamptz
);
CREATE INDEX list_items_org_kind_idx ON list_items (organization_id, kind) WHERE deleted_at IS NULL;

CREATE TABLE clients (
    id              uuid PRIMARY KEY,
    organization_id uuid NOT NULL REFERENCES organizations(id),
    name            text NOT NULL,
    phone           text NOT NULL DEFAULT '',
    comment         text NOT NULL DEFAULT '',
    bonus_balance   numeric(18,2) NOT NULL DEFAULT 0 CHECK (bonus_balance >= 0),
    bonus_level     numeric(5,2),             -- % кэшбэка; NULL = по умолчанию (3)
    created_at      timestamptz NOT NULL DEFAULT now(),
    deleted_at      timestamptz
);
CREATE UNIQUE INDEX clients_phone_uniq ON clients (organization_id, phone) WHERE deleted_at IS NULL AND phone <> '';

-- Остаток товара в филиале (qty может быть отрицательным, если разрешена продажа без остатка).
CREATE TABLE store_stock (
    product_id uuid NOT NULL REFERENCES products(id),
    store_id   uuid NOT NULL REFERENCES stores(id),
    qty        numeric(18,4) NOT NULL DEFAULT 0,
    PRIMARY KEY (product_id, store_id)
);

-- Журнал движений: единственный источник правды об изменении остатков.
CREATE TABLE product_movements (
    id              uuid PRIMARY KEY,
    organization_id uuid NOT NULL REFERENCES organizations(id),
    product_id      uuid NOT NULL REFERENCES products(id),
    store_id        uuid NOT NULL REFERENCES stores(id),
    kind            text NOT NULL CHECK (kind IN
        ('sale','sale_deleted','complex_sale','writeoff','supply','manufacture','transfer','return','revision','manual')),
    delta           numeric(18,4) NOT NULL,
    qty_before      numeric(18,4) NOT NULL,
    ref_id          uuid,
    user_id         uuid REFERENCES users(id),
    created_at      timestamptz NOT NULL DEFAULT clock_timestamp()
);
CREATE INDEX product_movements_product_idx ON product_movements (product_id, store_id, created_at);
CREATE INDEX product_movements_ref_idx ON product_movements (ref_id);

CREATE TABLE sales (
    id                    uuid PRIMARY KEY,
    organization_id       uuid NOT NULL REFERENCES organizations(id),
    store_id              uuid NOT NULL REFERENCES stores(id),
    user_id               uuid NOT NULL REFERENCES users(id),
    client_id             uuid REFERENCES clients(id),
    purchase_method_id    uuid REFERENCES list_items(id),
    payment_method_id     uuid REFERENCES list_items(id),
    second_payment_method_id uuid REFERENCES list_items(id),
    first_payment_value   numeric(18,2),
    second_payment_value  numeric(18,2),
    subtotal              numeric(18,2) NOT NULL,
    markup_amount         numeric(18,2) NOT NULL DEFAULT 0,
    discount_amount       numeric(18,2) NOT NULL DEFAULT 0,
    discount_percentage   numeric(7,3)  NOT NULL DEFAULT 0,
    bonus_reduction       numeric(18,2) NOT NULL DEFAULT 0,
    bonus_accumulation    numeric(18,2) NOT NULL DEFAULT 0,
    total                 numeric(18,2) NOT NULL,
    cost_total            numeric(18,4) NOT NULL DEFAULT 0,
    returned_amount       numeric(18,2) NOT NULL DEFAULT 0,
    delivery              boolean NOT NULL DEFAULT false,
    comment               text NOT NULL DEFAULT '',
    idempotency_key       text,
    sale_date             timestamptz NOT NULL DEFAULT now(),
    created_at            timestamptz NOT NULL DEFAULT now(),
    deleted_at            timestamptz
);
CREATE INDEX sales_org_date_idx ON sales (organization_id, store_id, sale_date) WHERE deleted_at IS NULL;
CREATE UNIQUE INDEX sales_idem_uniq ON sales (organization_id, idempotency_key) WHERE idempotency_key IS NOT NULL;

CREATE TABLE sale_items (
    id             uuid PRIMARY KEY,
    sale_id        uuid NOT NULL REFERENCES sales(id) ON DELETE CASCADE,
    product_id     uuid REFERENCES products(id),     -- NULL = кастомная позиция
    name           text NOT NULL,
    qty            numeric(18,4) NOT NULL CHECK (qty > 0),
    price          numeric(18,4) NOT NULL CHECK (price >= 0),
    cost_price     numeric(18,4) NOT NULL DEFAULT 0,
    returned_qty   numeric(18,4) NOT NULL DEFAULT 0
);
CREATE INDEX sale_items_sale_idx ON sale_items (sale_id);

CREATE TABLE sale_returns (
    id              uuid PRIMARY KEY,
    organization_id uuid NOT NULL REFERENCES organizations(id),
    sale_id         uuid NOT NULL REFERENCES sales(id),
    user_id         uuid NOT NULL REFERENCES users(id),
    return_type     text NOT NULL CHECK (return_type IN ('money','exchange')),
    restock         boolean NOT NULL,
    reason_id       uuid REFERENCES list_items(id),
    comment         text NOT NULL DEFAULT '',
    amount          numeric(18,2) NOT NULL,
    created_at      timestamptz NOT NULL DEFAULT now()
);
CREATE TABLE sale_return_items (
    return_id    uuid NOT NULL REFERENCES sale_returns(id) ON DELETE CASCADE,
    sale_item_id uuid NOT NULL REFERENCES sale_items(id),
    qty          numeric(18,4) NOT NULL CHECK (qty > 0),
    PRIMARY KEY (return_id, sale_item_id)
);

-- Отложенные чеки («Открытые чеки»).
CREATE TABLE saved_carts (
    id              uuid PRIMARY KEY,
    organization_id uuid NOT NULL REFERENCES organizations(id),
    store_id        uuid NOT NULL REFERENCES stores(id),
    user_id         uuid NOT NULL REFERENCES users(id),
    name            text NOT NULL DEFAULT '',
    payload         jsonb NOT NULL,
    created_at      timestamptz NOT NULL DEFAULT now()
);
