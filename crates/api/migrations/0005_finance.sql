-- Финансы: денежные счета, расходы, ежемесячные расходы.
CREATE TABLE wallets (
    id              uuid PRIMARY KEY,
    organization_id uuid NOT NULL REFERENCES organizations(id),
    store_id        uuid REFERENCES stores(id),        -- NULL = общий счёт
    name            text NOT NULL,
    created_at      timestamptz NOT NULL DEFAULT now(),
    deleted_at      timestamptz
);

-- Баланс счёта = сумма движений; удаление документа добавляет обратную запись.
CREATE TABLE wallet_transactions (
    id              uuid PRIMARY KEY,
    organization_id uuid NOT NULL REFERENCES organizations(id),
    wallet_id       uuid NOT NULL REFERENCES wallets(id),
    delta           numeric(18,2) NOT NULL,
    kind            text NOT NULL CHECK (kind IN ('spending','supply_payment','manual','transfer')),
    ref_id          uuid,
    comment         text NOT NULL DEFAULT '',
    user_id         uuid REFERENCES users(id),
    created_at      timestamptz NOT NULL DEFAULT clock_timestamp()
);
CREATE INDEX wallet_transactions_wallet_idx ON wallet_transactions (wallet_id, created_at);
CREATE INDEX wallet_transactions_ref_idx ON wallet_transactions (ref_id);

CREATE TABLE spendings (
    id              uuid PRIMARY KEY,
    organization_id uuid NOT NULL REFERENCES organizations(id),
    store_id        uuid NOT NULL REFERENCES stores(id),
    user_id         uuid NOT NULL REFERENCES users(id),
    category_id     uuid REFERENCES list_items(id),
    details         text NOT NULL DEFAULT '',
    amount          numeric(18,2) NOT NULL CHECK (amount > 0),
    wallet_id       uuid REFERENCES wallets(id),
    spending_date   date NOT NULL DEFAULT current_date,
    kind            text NOT NULL DEFAULT 'operating' CHECK (kind IN ('operating','capital')),
    created_at      timestamptz NOT NULL DEFAULT now(),
    deleted_at      timestamptz
);
CREATE INDEX spendings_org_idx ON spendings (organization_id, store_id, spending_date) WHERE deleted_at IS NULL;

CREATE TABLE recurring_expenses (
    id              uuid PRIMARY KEY,
    organization_id uuid NOT NULL REFERENCES organizations(id),
    store_id        uuid NOT NULL REFERENCES stores(id),
    name            text NOT NULL,
    amount          numeric(18,2) NOT NULL CHECK (amount > 0),
    start_date      date NOT NULL,
    end_date        date,
    created_at      timestamptz NOT NULL DEFAULT now(),
    deleted_at      timestamptz,
    CHECK (end_date IS NULL OR end_date >= start_date)
);

ALTER TABLE supplies ADD COLUMN wallet_id uuid REFERENCES wallets(id);
