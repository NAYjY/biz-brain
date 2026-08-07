-- S06 cleanup: 0006_actors.sql put channel/external_id directly on
-- workers/suppliers. That's superseded by actor_directory (019).
-- This migration adds proper branch FK and removes the identity cols
-- so workers/suppliers are pure domain entities.
--
-- If 0006_actors.sql was never run (fresh DB), this is a no-op ALTER;
-- run it safely regardless.

-- Add branch_id FK if not already present (0006 had it without REFERENCES)
ALTER TABLE workers
    ADD COLUMN IF NOT EXISTS branch_id_fk UUID REFERENCES branches(id);

-- Remove channel identity cols — now owned by actor_directory
ALTER TABLE workers
    DROP COLUMN IF EXISTS channel,
    DROP COLUMN IF EXISTS external_id;

ALTER TABLE suppliers
    DROP COLUMN IF EXISTS channel,
    DROP COLUMN IF EXISTS external_id;

-- Ensure branch_id has the FK constraint
-- (0006 declared branch_id NOT NULL but without REFERENCES)
ALTER TABLE workers
    DROP CONSTRAINT IF EXISTS workers_branch_id_fkey;
ALTER TABLE workers
    ADD CONSTRAINT workers_branch_id_fkey
        FOREIGN KEY (branch_id) REFERENCES branches(id);

ALTER TABLE suppliers
    DROP CONSTRAINT IF EXISTS suppliers_branch_id_fkey;
ALTER TABLE suppliers
    ADD CONSTRAINT suppliers_branch_id_fkey
        FOREIGN KEY (branch_id) REFERENCES branches(id);

CREATE INDEX IF NOT EXISTS idx_workers_branch ON workers(branch_id);
CREATE INDEX IF NOT EXISTS idx_suppliers_branch ON suppliers(branch_id);
