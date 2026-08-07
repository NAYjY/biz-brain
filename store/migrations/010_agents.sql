-- CONTEXT: one Agent per Branch, enforced at DB level.
-- T03: Agent scoped to Branch; AgentName is cosmetic only.

CREATE TABLE agents (
    id          UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    branch_id   UUID        NOT NULL UNIQUE REFERENCES branches(id), -- one per Branch
    agent_name  VARCHAR(255) NOT NULL,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
