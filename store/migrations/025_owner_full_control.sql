-- P16: Owner full order control.
-- Extends order_events CHECK to include all new force-state variants.
-- Adds owner_note column for audit trail on forced transitions.
-- Adds description mutability via a separate orders_edits log.

ALTER TABLE order_events
    DROP CONSTRAINT order_events_type_check,
    ADD CONSTRAINT order_events_type_check CHECK (
        event_type IN (
            'worker_assigned', 'worker_accepted', 'worker_unavailable',
            'worker_cancelled', 'clarification_requested',
            'worker_ready_for_pickup', 'order_done',
            'owner_cancelled', 'order_reset', 'clarification_resolved',
            -- P16: new force-state events (Owner bypass)
            'owner_force_accepted',
            'owner_force_unavailable',
            'owner_force_clarification',
            'owner_force_ready',
            'owner_reassign_worker'
        )
    );

-- Relax worker_id constraint: force events may or may not carry a worker_id.
-- owner_force_accepted / owner_force_clarification / owner_force_ready keep
-- the existing worker; owner_force_unavailable and owner_reassign_worker
-- always carry a worker_id (new or existing).
ALTER TABLE order_events
    DROP CONSTRAINT IF EXISTS chk_order_events_worker_id,
    ADD CONSTRAINT chk_order_events_worker_id CHECK (
        event_type IN ('order_done', 'order_reset','owner_cancelled',
                       'owner_force_accepted', 'owner_force_clarification',
                       'owner_force_ready')
        OR worker_id IS NOT NULL
    );

-- Optional free-text note on any order event (for Owner audit trail).
ALTER TABLE order_events
    ADD COLUMN IF NOT EXISTS owner_note VARCHAR(500);

-- Track description edits on orders (immutable event log, not UPDATE).
CREATE TABLE IF NOT EXISTS order_description_edits (
    id          BIGSERIAL   PRIMARY KEY,
    order_id    UUID        NOT NULL REFERENCES orders(id),
    new_description VARCHAR(1000) NOT NULL,
    edited_at   TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX IF NOT EXISTS idx_ode_order ON order_description_edits(order_id, id DESC);

-- Add description column to order projection so edits are reflected immediately.
ALTER TABLE order_current_state
    ADD COLUMN IF NOT EXISTS description VARCHAR(1000);

-- Soft-delete flag on orders (replaces hard delete for safety).
ALTER TABLE orders
    ADD COLUMN IF NOT EXISTS deleted_at TIMESTAMPTZ;

-- Hide deleted orders from all projection queries via a view.
CREATE OR REPLACE VIEW active_orders AS
    SELECT * FROM orders WHERE deleted_at IS NULL;
