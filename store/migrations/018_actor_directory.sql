-- S06: ChannelIdentity binding root-of-trust table.
-- T04 resolution: domain owns shape, store owns lookup data.
-- owner_confirmed = FALSE rows treated as unrecognized (fail closed).
-- Rebinding requires Owner action, not silent overwrite.

CREATE TABLE IF NOT EXISTS actor_directory (
    id              UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    channel         VARCHAR(16) NOT NULL CHECK (channel IN ('line', 'whats_app')),
    external_id     VARCHAR(255) NOT NULL,
    actor_type      VARCHAR(16) NOT NULL CHECK (actor_type IN ('worker', 'supplier')),
    actor_id        UUID        NOT NULL,   -- WorkerId or SupplierId
    branch_id       UUID        NOT NULL REFERENCES branches(id),
    -- S06: binding must be Owner-confirmed before trusted
    owner_confirmed BOOLEAN     NOT NULL DEFAULT FALSE,
    confirmed_at    TIMESTAMPTZ,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),

    UNIQUE (channel, external_id)  -- one channel identity -> one actor only
);

CREATE INDEX IF NOT EXISTS idx_actor_dir_lookup
    ON actor_directory(channel, external_id)
    WHERE owner_confirmed = TRUE;   -- only confirmed bindings in hot-path index

CREATE INDEX IF NOT EXISTS idx_actor_dir_actor
    ON actor_directory(actor_id, actor_type);