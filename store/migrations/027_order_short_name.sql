-- F01: Order Short Name — human-usable identifier for workers.
-- Unique per branch (partial index: NULL values are not considered duplicates).
-- Falls back to first 30 chars of description in bot messages when NULL.

ALTER TABLE orders
    ADD COLUMN IF NOT EXISTS short_name VARCHAR(20);

CREATE UNIQUE INDEX idx_orders_short_name_branch
    ON orders(branch_id, short_name)
    WHERE short_name IS NOT NULL;

-- Mirror on the projection so the dashboard read path never needs a join.
ALTER TABLE order_current_state
    ADD COLUMN IF NOT EXISTS short_name VARCHAR(20);
