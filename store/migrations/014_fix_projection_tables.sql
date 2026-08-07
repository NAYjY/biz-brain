-- 0003_projection_tables.sql created order_current_state and
-- supply_request_current_state without FK constraints (referenced tables
-- didn't exist yet). Add proper FKs and missing columns now.

-- order_current_state: add customer_id + worker_id columns (D04 needs them)
ALTER TABLE order_current_state
    RENAME COLUMN id TO order_id;

ALTER TABLE order_current_state
    ADD COLUMN IF NOT EXISTS customer_id  UUID REFERENCES customers(id),
    ADD COLUMN IF NOT EXISTS worker_id    UUID REFERENCES workers(id),
    ADD COLUMN IF NOT EXISTS last_event_type VARCHAR(64),
    ADD COLUMN IF NOT EXISTS last_event_at   TIMESTAMPTZ;

ALTER TABLE order_current_state
    ADD CONSTRAINT ocs_order_id_fkey
        FOREIGN KEY (order_id) REFERENCES orders(id),
    ADD CONSTRAINT ocs_branch_id_fkey
        FOREIGN KEY (branch_id) REFERENCES branches(id);

-- supply_request_current_state: add supplier_id + invoice_id columns (D05)
ALTER TABLE supply_request_current_state
    RENAME COLUMN id TO supply_request_id;

ALTER TABLE supply_request_current_state
    ADD COLUMN IF NOT EXISTS supplier_id     UUID REFERENCES suppliers(id),
    ADD COLUMN IF NOT EXISTS invoice_id      UUID REFERENCES invoices(id),
    ADD COLUMN IF NOT EXISTS last_event_type VARCHAR(64),
    ADD COLUMN IF NOT EXISTS last_event_at   TIMESTAMPTZ;

ALTER TABLE supply_request_current_state
    ADD CONSTRAINT srcs_supply_request_id_fkey
        FOREIGN KEY (supply_request_id) REFERENCES supply_requests(id),
    ADD CONSTRAINT srcs_branch_id_fkey
        FOREIGN KEY (branch_id) REFERENCES branches(id);

-- Rebuild indexes with correct column names
DROP INDEX IF EXISTS idx_order_current_state_branch;
CREATE INDEX idx_ocs_branch ON order_current_state(branch_id);
CREATE INDEX idx_ocs_branch_state ON order_current_state(branch_id, state);

DROP INDEX IF EXISTS idx_supply_request_current_state_branch;
CREATE INDEX idx_srcs_branch ON supply_request_current_state(branch_id);
CREATE INDEX idx_srcs_branch_state ON supply_request_current_state(branch_id, state);
