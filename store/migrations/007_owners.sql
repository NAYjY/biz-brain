-- D02: Owner account table. Single row per deployment under D01's
-- single-tenant model, but a real table (not a config value) so
-- multi-tenant adoption later needs no schema change.
-- S04: token_version embedded here from the start (avoids needing 017).

CREATE TABLE owners (
    id              UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    email           VARCHAR(255) NOT NULL UNIQUE,
    password_hash   VARCHAR(255) NOT NULL,          -- bcrypt
    token_version   INTEGER     NOT NULL DEFAULT 0, -- S04: bump on logout
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
