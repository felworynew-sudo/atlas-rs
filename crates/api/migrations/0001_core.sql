-- Ядро: организации, филиалы, роли, пользователи.
CREATE TABLE organizations (
    id              uuid PRIMARY KEY,
    name            text        NOT NULL,
    currency        text        NOT NULL DEFAULT 'RUB',
    price_decimals  integer     NOT NULL DEFAULT 0 CHECK (price_decimals BETWEEN 0 AND 4),
    modules         jsonb       NOT NULL,
    created_at      timestamptz NOT NULL DEFAULT now(),
    deleted_at      timestamptz
);

CREATE TABLE stores (
    id                uuid PRIMARY KEY,
    organization_id   uuid        NOT NULL REFERENCES organizations(id),
    name              text        NOT NULL,
    address           text        NOT NULL DEFAULT '',
    auto_close_time   text        NOT NULL DEFAULT '00:00',
    timezone          text        NOT NULL DEFAULT 'Europe/Moscow',
    show_stats_on_home boolean    NOT NULL DEFAULT true,
    fiscal_provider   text        NOT NULL DEFAULT 'none' CHECK (fiscal_provider IN ('none','light_kassa','wipon_kassa')),
    created_at        timestamptz NOT NULL DEFAULT now(),
    deleted_at        timestamptz
);
CREATE INDEX stores_org_idx ON stores (organization_id) WHERE deleted_at IS NULL;

CREATE TABLE roles (
    id              uuid PRIMARY KEY,
    organization_id uuid   NOT NULL REFERENCES organizations(id),
    key             text   NOT NULL,
    name            text   NOT NULL,
    salary_type     text   NOT NULL DEFAULT 'none' CHECK (salary_type IN ('none','hour','day','month')),
    salary_rate     numeric(18,2) NOT NULL DEFAULT 0,
    bonus_type      text   NOT NULL DEFAULT 'none' CHECK (bonus_type IN ('none','percent_own_sales')),
    permissions     text[] NOT NULL DEFAULT '{}',
    created_at      timestamptz NOT NULL DEFAULT now(),
    deleted_at      timestamptz,
    UNIQUE (organization_id, key)
);

CREATE TABLE users (
    id              uuid PRIMARY KEY,
    organization_id uuid   NOT NULL REFERENCES organizations(id),
    email           text   NOT NULL,
    name            text   NOT NULL,
    password_hash   text   NOT NULL,
    role_id         uuid   NOT NULL REFERENCES roles(id),
    store_ids       uuid[] NOT NULL DEFAULT '{}',
    active          boolean NOT NULL DEFAULT true,
    created_at      timestamptz NOT NULL DEFAULT now()
);
CREATE UNIQUE INDEX users_email_uniq ON users (lower(email));
CREATE INDEX users_org_idx ON users (organization_id);
