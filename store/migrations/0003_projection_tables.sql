-- T02: materialized current-state projections. Async worker updates these;
-- dashboard (api crate, T05) reads these, never the raw event stream.

CREATE TABLE order_current_state (
    id              UUID PRIMARY KEY,
    branch_id       UUID NOT NULL,
    customer_id     UUID NOT NULL,
    description     VARCHAR(1000) NOT NULL,
    state           VARCHAR(30) NOT NULL,
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_order_current_state_branch ON order_current_state (branch_id, state);

CREATE TABLE supply_request_current_state (
    id              UUID PRIMARY KEY,
    branch_id       UUID NOT NULL,
    order_ids       JSONB NOT NULL DEFAULT '[]', -- denormalized display data only, not event storage
    description     VARCHAR(1000) NOT NULL,
    state           VARCHAR(30) NOT NULL,
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_supply_request_current_state_branch ON supply_request_current_state (branch_id, state);
