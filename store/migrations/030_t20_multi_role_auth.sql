-- T20: Branch Management & Multi-Role Auth
--
-- Replaces the single `owners` table with a two-tier model:
--   Owner   → users WHERE role = 'owner'  (all branches implicitly)
--   Manager → users WHERE role = 'manager' + user_branch_access rows

-- 1. Rename the existing table and add the role column.
ALTER TABLE owners RENAME TO users;

ALTER TABLE users
    ADD COLUMN IF NOT EXISTS role VARCHAR(16) NOT NULL DEFAULT 'owner'
        CHECK (role IN ('owner', 'manager'));

-- 2. branches.owner_id → created_by_user_id
DO $$
BEGIN
    IF EXISTS (
        SELECT 1 FROM information_schema.columns
        WHERE table_name = 'branches' AND column_name = 'owner_id'
    ) THEN
        ALTER TABLE branches RENAME COLUMN owner_id TO created_by_user_id;
    END IF;
END $$;

-- Drop old FK if it exists, recreate with predictable name.
ALTER TABLE branches
    DROP CONSTRAINT IF EXISTS branches_owner_id_fkey;

ALTER TABLE branches
    DROP CONSTRAINT IF EXISTS branches_created_by_user_id_fkey;

ALTER TABLE branches
    ADD CONSTRAINT branches_created_by_user_id_fkey
        FOREIGN KEY (created_by_user_id) REFERENCES users(id);

-- 3. Branch access table for Managers.
CREATE TABLE IF NOT EXISTS user_branch_access (
    user_id     UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    branch_id   UUID NOT NULL REFERENCES branches(id) ON DELETE CASCADE,
    granted_by  UUID REFERENCES users(id),
    granted_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (user_id, branch_id)
);

CREATE INDEX IF NOT EXISTS idx_uba_user   ON user_branch_access(user_id);
CREATE INDEX IF NOT EXISTS idx_uba_branch ON user_branch_access(branch_id);

-- 4. Update index.
CREATE UNIQUE INDEX IF NOT EXISTS idx_users_email ON users(email);

-- 5. token_version guard (safe no-op if already present).
ALTER TABLE users
    ADD COLUMN IF NOT EXISTS token_version INTEGER NOT NULL DEFAULT 0;