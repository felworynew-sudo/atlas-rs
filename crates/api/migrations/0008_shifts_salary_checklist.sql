-- Смены, касса, рабочие часы, зарплата, чеклист.
ALTER TABLE roles ADD COLUMN bonus_rate numeric(5,2) NOT NULL DEFAULT 0 CHECK (bonus_rate BETWEEN 0 AND 100);
ALTER TABLE wallets ADD COLUMN is_cash_register boolean NOT NULL DEFAULT false;   -- «Наличные с кассы <филиал>»

CREATE TABLE shifts (
    id              uuid PRIMARY KEY,
    organization_id uuid NOT NULL REFERENCES organizations(id),
    store_id        uuid NOT NULL REFERENCES stores(id),
    opened_by       uuid NOT NULL REFERENCES users(id),
    opened_at       timestamptz NOT NULL DEFAULT now(),
    closed_at       timestamptz,
    closed_by       uuid REFERENCES users(id),
    opening_cash    numeric(18,2) NOT NULL DEFAULT 0,
    expected_cash   numeric(18,2),
    actual_cash     numeric(18,2),
    diff            numeric(18,2),                -- факт − расчёт: недосдача < 0, излишек > 0
    comment         text NOT NULL DEFAULT '',
    auto_closed     boolean NOT NULL DEFAULT false
);
CREATE UNIQUE INDEX shifts_one_open_per_store ON shifts (store_id) WHERE closed_at IS NULL;
CREATE INDEX shifts_org_idx ON shifts (organization_id, store_id, opened_at);

CREATE TABLE shift_staff (
    shift_id uuid NOT NULL REFERENCES shifts(id) ON DELETE CASCADE,
    user_id  uuid NOT NULL REFERENCES users(id),
    added_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (shift_id, user_id)
);

CREATE TABLE cashbox_transactions (
    id              uuid PRIMARY KEY,
    organization_id uuid NOT NULL REFERENCES organizations(id),
    shift_id        uuid NOT NULL REFERENCES shifts(id) ON DELETE CASCADE,
    user_id         uuid NOT NULL REFERENCES users(id),
    amount          numeric(18,2) NOT NULL CHECK (amount <> 0),     -- внесение > 0, изъятие < 0
    comment         text NOT NULL DEFAULT '',
    created_at      timestamptz NOT NULL DEFAULT now()
);

ALTER TABLE sales ADD COLUMN shift_id uuid REFERENCES shifts(id);

CREATE TABLE working_hours (
    id              uuid PRIMARY KEY,
    organization_id uuid NOT NULL REFERENCES organizations(id),
    store_id        uuid NOT NULL REFERENCES stores(id),
    user_id         uuid NOT NULL REFERENCES users(id),
    shift_id        uuid REFERENCES shifts(id),
    started_at      timestamptz NOT NULL,
    ended_at        timestamptz,
    created_by      uuid REFERENCES users(id),
    CHECK (ended_at IS NULL OR ended_at >= started_at)
);
CREATE INDEX working_hours_user_idx ON working_hours (organization_id, user_id, started_at);

CREATE TABLE salary_payments (
    id              uuid PRIMARY KEY,
    organization_id uuid NOT NULL REFERENCES organizations(id),
    user_id         uuid NOT NULL REFERENCES users(id),
    amount          numeric(18,2) NOT NULL CHECK (amount > 0),
    wallet_id       uuid REFERENCES wallets(id),
    comment         text NOT NULL DEFAULT '',
    paid_on         date NOT NULL DEFAULT current_date,
    created_by      uuid REFERENCES users(id),
    created_at      timestamptz NOT NULL DEFAULT now(),
    deleted_at      timestamptz
);

CREATE TABLE checklist_items (
    id              uuid PRIMARY KEY,
    organization_id uuid NOT NULL REFERENCES organizations(id),
    store_id        uuid NOT NULL REFERENCES stores(id),
    title           text NOT NULL,
    time_of_day     text NOT NULL DEFAULT '',        -- «ЧЧ:ММ» или пусто
    sort_order      integer NOT NULL DEFAULT 0,
    created_at      timestamptz NOT NULL DEFAULT now(),
    deleted_at      timestamptz
);
CREATE TABLE checklist_marks (
    item_id   uuid NOT NULL REFERENCES checklist_items(id) ON DELETE CASCADE,
    mark_date date NOT NULL,
    user_id   uuid NOT NULL REFERENCES users(id),
    done_at   timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (item_id, mark_date)
);
