-- Каталог: категории, поставщики, единицы, товары (цены по филиалам, штрихкоды, состав).
CREATE TABLE product_categories (
    id              uuid PRIMARY KEY,
    organization_id uuid    NOT NULL REFERENCES organizations(id),
    name            text    NOT NULL,
    sort_order      integer NOT NULL DEFAULT 0,
    created_at      timestamptz NOT NULL DEFAULT now(),
    deleted_at      timestamptz
);
CREATE INDEX product_categories_org_idx ON product_categories (organization_id) WHERE deleted_at IS NULL;

CREATE TABLE suppliers (
    id              uuid PRIMARY KEY,
    organization_id uuid NOT NULL REFERENCES organizations(id),
    name            text NOT NULL,
    created_at      timestamptz NOT NULL DEFAULT now(),
    deleted_at      timestamptz
);
CREATE INDEX suppliers_org_idx ON suppliers (organization_id) WHERE deleted_at IS NULL;

CREATE TABLE product_units (
    id              uuid PRIMARY KEY,
    organization_id uuid NOT NULL REFERENCES organizations(id),
    name            text NOT NULL,
    short_name      text NOT NULL DEFAULT '',
    created_at      timestamptz NOT NULL DEFAULT now(),
    deleted_at      timestamptz
);
CREATE INDEX product_units_org_idx ON product_units (organization_id) WHERE deleted_at IS NULL;

CREATE TABLE products (
    id                  uuid PRIMARY KEY,
    organization_id     uuid NOT NULL REFERENCES organizations(id),
    name                text NOT NULL,
    is_service          boolean NOT NULL DEFAULT false,
    is_composite        boolean NOT NULL DEFAULT false,
    -- 'ahead' = производится заранее, 'on_sale' = во время продажи (остаток всегда 0)
    production_type     text NOT NULL DEFAULT 'ahead' CHECK (production_type IN ('ahead','on_sale')),
    hidden_in_new_sale  boolean NOT NULL DEFAULT false,
    purchase_price      numeric(18,4) NOT NULL DEFAULT 0 CHECK (purchase_price >= 0),
    extra_cost          numeric(18,4) NOT NULL DEFAULT 0 CHECK (extra_cost >= 0),
    category_id         uuid REFERENCES product_categories(id),
    supplier_id         uuid REFERENCES suppliers(id),
    unit_id             uuid REFERENCES product_units(id),
    barcodes            text[] NOT NULL DEFAULT '{}',
    allow_fractional    boolean NOT NULL DEFAULT false,
    important           boolean NOT NULL DEFAULT false,
    stock_threshold     numeric(18,4) NOT NULL DEFAULT 0 CHECK (stock_threshold >= 0),
    created_at          timestamptz NOT NULL DEFAULT now(),
    deleted_at          timestamptz
);
CREATE INDEX products_org_idx ON products (organization_id) WHERE deleted_at IS NULL;
CREATE INDEX products_barcodes_idx ON products USING gin (barcodes);

-- цена продажи и доступность товара по филиалам
CREATE TABLE product_stores (
    product_id  uuid NOT NULL REFERENCES products(id) ON DELETE CASCADE,
    store_id    uuid NOT NULL REFERENCES stores(id),
    active      boolean NOT NULL DEFAULT true,
    sale_price  numeric(18,4) NOT NULL DEFAULT 0 CHECK (sale_price >= 0),
    PRIMARY KEY (product_id, store_id)
);

-- рецептура составного товара
CREATE TABLE product_ingredients (
    product_id    uuid NOT NULL REFERENCES products(id) ON DELETE CASCADE,
    ingredient_id uuid NOT NULL REFERENCES products(id),
    qty           numeric(18,4) NOT NULL CHECK (qty > 0),
    PRIMARY KEY (product_id, ingredient_id),
    CHECK (product_id <> ingredient_id)
);
