-- Складские документы: приёмки, списания, ревизии, заказы поставщикам.
CREATE TABLE supply_orders (
    id              uuid PRIMARY KEY,
    organization_id uuid NOT NULL REFERENCES organizations(id),
    store_id        uuid NOT NULL REFERENCES stores(id),
    supplier_id     uuid REFERENCES suppliers(id),
    user_id         uuid NOT NULL REFERENCES users(id),
    status          text NOT NULL DEFAULT 'draft'
                    CHECK (status IN ('draft','ordered','partially_received','received','cancelled')),
    order_date      date NOT NULL DEFAULT current_date,
    comment         text NOT NULL DEFAULT '',
    created_at      timestamptz NOT NULL DEFAULT now(),
    deleted_at      timestamptz
);
CREATE TABLE supply_order_items (
    order_id   uuid NOT NULL REFERENCES supply_orders(id) ON DELETE CASCADE,
    product_id uuid NOT NULL REFERENCES products(id),
    qty        numeric(18,4) NOT NULL CHECK (qty > 0),
    unit_price numeric(18,4) NOT NULL DEFAULT 0 CHECK (unit_price >= 0),
    PRIMARY KEY (order_id, product_id)
);

CREATE TABLE supplies (
    id              uuid PRIMARY KEY,
    organization_id uuid NOT NULL REFERENCES organizations(id),
    store_id        uuid NOT NULL REFERENCES stores(id),
    supplier_id     uuid REFERENCES suppliers(id),
    order_id        uuid REFERENCES supply_orders(id),
    user_id         uuid NOT NULL REFERENCES users(id),
    supply_date     date NOT NULL DEFAULT current_date,
    has_return      boolean NOT NULL DEFAULT false,   -- возврат поставщику: остатки уменьшаются
    prices_changed  boolean NOT NULL DEFAULT false,   -- обновить закупочные цены товаров
    paid            boolean NOT NULL DEFAULT false,
    invoice_photo   text,
    comment         text NOT NULL DEFAULT '',
    total           numeric(18,2) NOT NULL DEFAULT 0,
    created_at      timestamptz NOT NULL DEFAULT now(),
    deleted_at      timestamptz
);
CREATE INDEX supplies_org_idx ON supplies (organization_id, store_id, supply_date) WHERE deleted_at IS NULL;
CREATE TABLE supply_items (
    id         uuid PRIMARY KEY,
    supply_id  uuid NOT NULL REFERENCES supplies(id) ON DELETE CASCADE,
    product_id uuid NOT NULL REFERENCES products(id),
    qty        numeric(18,4) NOT NULL CHECK (qty > 0),
    price      numeric(18,4) NOT NULL CHECK (price >= 0)
);

CREATE TABLE writeoffs (
    id              uuid PRIMARY KEY,
    organization_id uuid NOT NULL REFERENCES organizations(id),
    store_id        uuid NOT NULL REFERENCES stores(id),
    user_id         uuid NOT NULL REFERENCES users(id),
    product_id      uuid NOT NULL REFERENCES products(id),
    qty             numeric(18,4) NOT NULL CHECK (qty > 0),
    reason_id       uuid NOT NULL REFERENCES list_items(id),
    amount          numeric(18,2) NOT NULL DEFAULT 0,
    comment         text NOT NULL DEFAULT '',
    writeoff_date   timestamptz NOT NULL DEFAULT now(),
    created_at      timestamptz NOT NULL DEFAULT now(),
    deleted_at      timestamptz
);
CREATE INDEX writeoffs_org_idx ON writeoffs (organization_id, store_id, writeoff_date) WHERE deleted_at IS NULL;

CREATE TABLE revisions (
    id              uuid PRIMARY KEY,
    organization_id uuid NOT NULL REFERENCES organizations(id),
    store_id        uuid NOT NULL REFERENCES stores(id),
    category_id     uuid REFERENCES product_categories(id),
    user_id         uuid NOT NULL REFERENCES users(id),
    comment         text NOT NULL DEFAULT '',
    total_diff      numeric(18,2) NOT NULL DEFAULT 0,   -- Σ (факт − система) × себестоимость
    revision_date   timestamptz NOT NULL DEFAULT now(),
    created_at      timestamptz NOT NULL DEFAULT now(),
    deleted_at      timestamptz
);
CREATE TABLE revision_items (
    revision_id uuid NOT NULL REFERENCES revisions(id) ON DELETE CASCADE,
    product_id  uuid NOT NULL REFERENCES products(id),
    system_qty  numeric(18,4) NOT NULL,
    fact_qty    numeric(18,4) NOT NULL,
    diff_amount numeric(18,2) NOT NULL,
    PRIMARY KEY (revision_id, product_id)
);
