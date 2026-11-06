-- API-токены организации, журнал их использования и черновики, подготовленные ИИ-агентом.
CREATE TABLE api_tokens (
    id              uuid PRIMARY KEY,
    organization_id uuid NOT NULL REFERENCES organizations(id),
    name            text NOT NULL,
    prefix          text NOT NULL,                 -- первые символы секрета, показываются в интерфейсе
    token_hash      text NOT NULL UNIQUE,          -- SHA-256 секрета; сам секрет не хранится
    scopes          text[] NOT NULL,
    expires_at      timestamptz NOT NULL,
    created_by      uuid NOT NULL REFERENCES users(id),
    created_at      timestamptz NOT NULL DEFAULT now(),
    revoked_at      timestamptz,
    last_used_at    timestamptz
);
CREATE INDEX api_tokens_org_idx ON api_tokens (organization_id);

CREATE TABLE api_audit_events (
    id              uuid PRIMARY KEY,
    organization_id uuid NOT NULL REFERENCES organizations(id),
    token_id        uuid REFERENCES api_tokens(id),
    user_id         uuid REFERENCES users(id),
    action          text NOT NULL,
    allowed         boolean NOT NULL DEFAULT true,
    text            text NOT NULL,
    created_at      timestamptz NOT NULL DEFAULT clock_timestamp()
);
CREATE INDEX api_audit_org_idx ON api_audit_events (organization_id, created_at DESC);

CREATE TABLE api_drafts (
    id              uuid PRIMARY KEY,
    organization_id uuid NOT NULL REFERENCES organizations(id),
    token_id        uuid NOT NULL REFERENCES api_tokens(id),
    kind            text NOT NULL CHECK (kind IN ('sale','acceptance')),
    store_id        uuid NOT NULL REFERENCES stores(id),
    payload         jsonb NOT NULL,
    status          text NOT NULL DEFAULT 'proposed' CHECK (status IN ('proposed','applied','dismissed')),
    expires_at      timestamptz NOT NULL,
    result          jsonb,
    created_at      timestamptz NOT NULL DEFAULT now(),
    resolved_at     timestamptz
);
CREATE INDEX api_drafts_org_idx ON api_drafts (organization_id, created_at DESC);
