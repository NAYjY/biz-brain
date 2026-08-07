-- D04: Customer table. Added mid-ticket when it became clear no
-- Customer concept/endpoint existed yet. Deliberately minimal —
-- not a full profile/history feature. Owner creates Orders on
-- Customer behalf; Customer has no login, no write access.

CREATE TABLE customers (
    id          UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    branch_id   UUID        NOT NULL REFERENCES branches(id),
    name        VARCHAR(255) NOT NULL,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_customers_branch ON customers(branch_id);
