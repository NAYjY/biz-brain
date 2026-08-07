-- D02: Branch table. One-to-many owner → branches.
-- T05: JWT carries owned_branch_ids array; URL-scoped per request.

CREATE TABLE branches (
    id          UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    owner_id    UUID        NOT NULL REFERENCES owners(id),
    name        VARCHAR(255) NOT NULL,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_branches_owner ON branches(owner_id);
