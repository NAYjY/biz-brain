-- Worker/Supplier persisted identity. T04: `messaging` owns the *lookup*
-- from an inbound sender to (Channel, external_id) -> WorkerId/SupplierId;
-- `domain` owns the shape (ChannelIdentity). This table is what the lookup
-- actually queries.

CREATE TABLE workers (
    id              UUID PRIMARY KEY,
    branch_id       UUID NOT NULL,
    name            VARCHAR(255) NOT NULL,
    channel         VARCHAR(20) NOT NULL CHECK (channel IN ('line', 'whats_app')),
    external_id     VARCHAR(255) NOT NULL,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),

    CONSTRAINT uq_workers_identity UNIQUE (channel, external_id)
);

CREATE TABLE suppliers (
    id              UUID PRIMARY KEY,
    branch_id       UUID NOT NULL,
    name            VARCHAR(255) NOT NULL,
    channel         VARCHAR(20) NOT NULL CHECK (channel IN ('line', 'whats_app')),
    external_id     VARCHAR(255) NOT NULL,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),

    CONSTRAINT uq_suppliers_identity UNIQUE (channel, external_id)
);
