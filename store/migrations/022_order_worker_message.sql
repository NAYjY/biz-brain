-- Store last raw Worker message text on the order projection so
-- Owner can see what Worker said and reply from dashboard.
ALTER TABLE order_current_state
    ADD COLUMN IF NOT EXISTS last_worker_message      TEXT,
    ADD COLUMN IF NOT EXISTS last_worker_message_at   TIMESTAMPTZ;
