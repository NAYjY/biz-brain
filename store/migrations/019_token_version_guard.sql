-- S04: token_version was included in 007_owners.sql from the start.
-- This migration is a safety guard in case owners was created without it
-- (e.g. via the old D02 seed script which predated S04).

ALTER TABLE owners
    ADD COLUMN IF NOT EXISTS token_version INTEGER NOT NULL DEFAULT 0;
