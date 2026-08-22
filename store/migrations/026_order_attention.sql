-- F04: Thread modal + attention signals.
-- Adds unread worker message count and AI low-confidence routing flag to the
-- order projection so the dashboard can surface attention signals without
-- re-querying conversation_history on every list load.

ALTER TABLE order_current_state
    ADD COLUMN IF NOT EXISTS unread_message_count    INTEGER NOT NULL DEFAULT 0,
    ADD COLUMN IF NOT EXISTS ai_routed_low_confidence BOOLEAN NOT NULL DEFAULT FALSE;

-- last_event_at was added in 014 (last_event_at TIMESTAMPTZ); ensure it exists.
-- Safe no-op if already present.
ALTER TABLE order_current_state
    ADD COLUMN IF NOT EXISTS last_event_at TIMESTAMPTZ;
