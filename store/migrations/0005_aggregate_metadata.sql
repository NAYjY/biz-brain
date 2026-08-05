-- Order/SupplyRequest creation is an Owner command, not a DomainEvent (T01's
-- Domain Events list has no OrderCreated/SupplyRequestCreated variant) — so
-- static creation-time fields live here, separate from the event streams.

CREATE TABLE orders (
    id              UUID PRIMARY KEY,
    branch_id       UUID NOT NULL,
    customer_id     UUID NOT NULL,
    description     VARCHAR(1000) NOT NULL,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE supply_requests (
    id              UUID PRIMARY KEY,
    branch_id       UUID NOT NULL,
    description     VARCHAR(1000) NOT NULL,
    order_ids       JSONB NOT NULL DEFAULT '[]',
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
