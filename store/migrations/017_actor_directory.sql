-- S06: ChannelIdentity binding root-of-trust table.
-- Supersedes channel/external_id cols on workers/suppliers (see 011).
-- owner_confirmed = FALSE rows are invisible to the trusted lookup path.

CREATE TABLE actor_directory (
    id              UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    channel         VARCHAR(16) NOT NULL CHECK (channel IN ('line', 'whats_app')),
    external_id     VARCHAR(255) NOT NULL,
    actor_type      VARCHAR(16) NOT NULL CHECK (actor_type IN ('worker', 'supplier')),
    actor_id        UUID        NOT NULL,
    branch_id       UUID        NOT NULL REFERENCES branches(id),
    owner_confirmed BOOLEAN     NOT NULL DEFAULT FALSE,
    confirmed_at    TIMESTAMPTZ,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),

    UNIQUE (channel, external_id)
);

-- Hot-path index: only confirmed rows visible to trusted lookup
CREATE INDEX idx_actor_dir_lookup
    ON actor_directory(channel, external_id)
    WHERE owner_confirmed = TRUE;

CREATE INDEX idx_actor_dir_actor
    ON actor_directory(actor_id, actor_type);
