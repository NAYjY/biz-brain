-- F05: Order start/due dates and follow-up alerts.
--
-- start_date / due_date: optional, stored on orders table (source of truth)
-- and mirrored to order_current_state (fast read path).
--
-- follow_up_alerts: separate table — one-or-many alerts per order.
-- alert_mode:
--   'once'    — fires at alert_at (absolute datetime), then soft-deleted
--   'hourly'  — fires every hour starting from alert_at
--   'daily'   — fires every 24 hours starting from alert_at
--   'every3d' — fires every 72 hours starting from alert_at

ALTER TABLE orders
    ADD COLUMN IF NOT EXISTS start_date  TIMESTAMPTZ,
    ADD COLUMN IF NOT EXISTS due_date    TIMESTAMPTZ;

ALTER TABLE order_current_state
    ADD COLUMN IF NOT EXISTS start_date  TIMESTAMPTZ,
    ADD COLUMN IF NOT EXISTS due_date    TIMESTAMPTZ;

CREATE TABLE follow_up_alerts (
    id          UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    order_id    UUID        NOT NULL REFERENCES orders(id),
    branch_id   UUID        NOT NULL REFERENCES branches(id),
    alert_mode  VARCHAR(16) NOT NULL
                CHECK (alert_mode IN ('once', 'hourly', 'daily', 'every3d')),
    alert_at    TIMESTAMPTZ NOT NULL,       -- absolute fire time (or first fire for recurring)
    next_fire_at TIMESTAMPTZ NOT NULL,      -- updated after each fire for recurring
    message     TEXT,                       -- optional custom message; NULL = default
    fired_count INTEGER     NOT NULL DEFAULT 0,
    deleted_at  TIMESTAMPTZ,               -- soft-delete on 'once' after fire / Owner clears
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_fua_next_fire
    ON follow_up_alerts(next_fire_at)
    WHERE deleted_at IS NULL;

CREATE INDEX idx_fua_order
    ON follow_up_alerts(order_id)
    WHERE deleted_at IS NULL;