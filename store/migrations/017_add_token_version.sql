-- S04: session revocation via token_version.
-- Bump token_version on logout/password-change -> stale JWTs rejected
-- even if sig + exp still valid. Single-tenant = one owners row, so
-- the DB lookup per request is negligible cost (S04 resolution).

ALTER TABLE owners
    ADD COLUMN IF NOT EXISTS token_version INTEGER NOT NULL DEFAULT 0;