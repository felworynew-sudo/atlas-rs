-- Перемещения, производство, платежи, обновления, этикетки; детали закрытия смены и виды пунктов чек-листа.
ALTER TABLE shifts ADD COLUMN close_details jsonb;          -- закрытие по способам оплаты: [{paymentMethodId, expected, actual, diff}]
ALTER TABLE checklist_items ADD COLUMN kind text NOT NULL DEFAULT 'daily' CHECK (kind IN ('daily','once','section'));

ALTER TABLE organizations ADD COLUMN label_settings jsonb NOT NULL DEFAULT
    '{"labelHeight":112,"fontSize":10,"priceFontSize":12,"printDate":true}';

CREATE TABLE transfers (
    id              uuid PRIMARY KEY,
    organization_id uuid NOT NULL REFERENCES organizations(id),
    from_store_id   uuid NOT NULL REFERENCES stores(id),
    to_store_id     uuid NOT NULL REFERENCES stores(id),
    user_id         uuid NOT NULL REFERENCES users(id),
    transfer_date   timestamptz NOT NULL DEFAULT now(),
    comment         text NOT NULL DEFAULT '',
    amount          numeric(18,2) NOT NULL DEFAULT 0,       -- сумма по себестоимости
    created_at      timestamptz NOT NULL DEFAULT now(),
    deleted_at      timestamptz,
    CHECK (from_store_id <> to_store_id)
);
CREATE INDEX transfers_org_idx ON transfers (organization_id, transfer_date) WHERE deleted_at IS NULL;
CREATE TABLE transfer_items (
    transfer_id uuid NOT NULL REFERENCES transfers(id) ON DELETE CASCADE,
    product_id  uuid NOT NULL REFERENCES products(id),
    qty         numeric(18,4) NOT NULL CHECK (qty > 0),
    PRIMARY KEY (transfer_id, product_id)
);

CREATE TABLE manufactures (
    id               uuid PRIMARY KEY,
    organization_id  uuid NOT NULL REFERENCES organizations(id),
    store_id         uuid NOT NULL REFERENCES stores(id),
    product_id       uuid NOT NULL REFERENCES products(id),
    user_id          uuid NOT NULL REFERENCES users(id),
    qty              numeric(18,4) NOT NULL CHECK (qty > 0),
    batch            text NOT NULL DEFAULT '',
    manufacture_date timestamptz NOT NULL DEFAULT now(),
    cost_total       numeric(18,2) NOT NULL DEFAULT 0,
    created_at       timestamptz NOT NULL DEFAULT now(),
    deleted_at       timestamptz
);
CREATE INDEX manufactures_org_idx ON manufactures (organization_id, store_id, manufacture_date) WHERE deleted_at IS NULL;
CREATE TABLE manufacture_ingredients (
    manufacture_id uuid NOT NULL REFERENCES manufactures(id) ON DELETE CASCADE,
    product_id     uuid NOT NULL REFERENCES products(id),
    qty            numeric(18,4) NOT NULL CHECK (qty > 0),
    PRIMARY KEY (manufacture_id, product_id)
);

-- Денежные транзакции («Платежи»): оплата с денежного счёта за период.
CREATE TABLE money_payments (
    id              uuid PRIMARY KEY,
    organization_id uuid NOT NULL REFERENCES organizations(id),
    store_id        uuid NOT NULL REFERENCES stores(id),
    user_id         uuid NOT NULL REFERENCES users(id),
    category_id     uuid REFERENCES list_items(id),
    wallet_id       uuid NOT NULL REFERENCES wallets(id),
    amount          numeric(18,2) NOT NULL CHECK (amount > 0),
    paid_on         date NOT NULL DEFAULT current_date,
    period_from     date,
    period_to       date,
    details         text NOT NULL DEFAULT '',
    created_at      timestamptz NOT NULL DEFAULT now(),
    deleted_at      timestamptz,
    CHECK (period_to IS NULL OR period_from IS NULL OR period_to >= period_from)
);
CREATE INDEX money_payments_org_idx ON money_payments (organization_id, store_id, paid_on) WHERE deleted_at IS NULL;

-- Лента «Обновления».
CREATE TABLE update_events (
    id              uuid PRIMARY KEY,
    organization_id uuid NOT NULL REFERENCES organizations(id),
    store_id        uuid REFERENCES stores(id),
    user_id         uuid REFERENCES users(id),
    kind            text NOT NULL,
    text            text NOT NULL,
    created_at      timestamptz NOT NULL DEFAULT clock_timestamp()
);
CREATE INDEX update_events_org_idx ON update_events (organization_id, created_at DESC);
CREATE TABLE update_reads (
    user_id uuid PRIMARY KEY REFERENCES users(id),
    read_at timestamptz NOT NULL DEFAULT now()
);
