-- T09: Pagination indexes.
-- Composite index matching the keyset cursor: (is_terminal, last_event_at DESC, order_id).
-- The IS TERMINAL expression is approximated with a CASE that matches the Rust sort logic.
-- pg_trgm extension for future full-text ILIKE performance; ILIKE falls back to seq-scan if
-- not installed — acceptable for initial launch (noted in T09 resolution as a follow-up).

-- Keyset cursor index for orders
CREATE INDEX IF NOT EXISTS idx_ocs_cursor
    ON order_current_state (
        (CASE WHEN state IN ('DONE','CANCELLED') THEN 1 ELSE 0 END) ASC,
        last_event_at DESC NULLS LAST,
        order_id ASC
    );

-- Keyset cursor index for supply requests
CREATE INDEX IF NOT EXISTS idx_srcs_cursor
    ON supply_request_current_state (
        (CASE WHEN state = 'SUPPLIER_CONFIRMED' THEN 1 ELSE 0 END) ASC,
        updated_at DESC NULLS LAST,
        supply_request_id ASC
    );

-- Optional: enable pg_trgm for future ILIKE performance (no-op if already enabled).
-- CREATE EXTENSION IF NOT EXISTS pg_trgm;
-- CREATE INDEX IF NOT EXISTS idx_orders_description_trgm ON orders USING gin (description gin_trgm_ops);
-- CREATE INDEX IF NOT EXISTS idx_orders_short_name_trgm  ON orders USING gin (short_name   gin_trgm_ops);