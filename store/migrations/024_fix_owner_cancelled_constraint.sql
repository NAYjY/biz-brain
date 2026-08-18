-- Fix: owner_cancelled carries no worker_id (same as order_done and order_reset)
-- but was missing from the exemption list added in 023.

ALTER TABLE order_events
    DROP CONSTRAINT IF EXISTS chk_order_events_worker_id,
    ADD CONSTRAINT chk_order_events_worker_id CHECK (
        event_type IN ('order_done', 'order_reset', 'owner_cancelled') OR worker_id IS NOT NULL
    );