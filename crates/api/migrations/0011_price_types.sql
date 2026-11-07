-- Ценовые категории (оптовые, по акции и т.п.), цены товаров по ним и предпочитаемая категория клиента.
CREATE TABLE price_types (
    id              uuid PRIMARY KEY,
    organization_id uuid NOT NULL REFERENCES organizations(id),
    name            text NOT NULL,
    -- store: отдельные цены по филиалам, common: общие цены
    scope           text NOT NULL DEFAULT 'store' CHECK (scope IN ('store', 'common')),
    -- авто-расчёт: базовая цена продажи × (1 + markup_percent/100); иначе цены задаются вручную
    auto_calc       boolean NOT NULL DEFAULT false,
    markup_percent  numeric(9,4) NOT NULL DEFAULT 0,
    -- правила авто-выбора: {"minQty": n, "minTotal": n, "paymentMethodId": uuid, "byClient": bool}
    rules           jsonb NOT NULL DEFAULT '{}',
    created_at      timestamptz NOT NULL DEFAULT now(),
    deleted_at      timestamptz
);
CREATE INDEX price_types_org ON price_types (organization_id) WHERE deleted_at IS NULL;

-- ручные цены; для общих категорий store_id = нулевой UUID
CREATE TABLE product_price_type_prices (
    price_type_id uuid NOT NULL REFERENCES price_types(id) ON DELETE CASCADE,
    product_id    uuid NOT NULL REFERENCES products(id) ON DELETE CASCADE,
    store_id      uuid NOT NULL DEFAULT '00000000-0000-0000-0000-000000000000',
    price         numeric(18,4) NOT NULL CHECK (price >= 0),
    PRIMARY KEY (price_type_id, product_id, store_id)
);

ALTER TABLE clients ADD COLUMN preferred_price_type_id uuid REFERENCES price_types(id);
ALTER TABLE sales ADD COLUMN price_type_id uuid REFERENCES price_types(id);
