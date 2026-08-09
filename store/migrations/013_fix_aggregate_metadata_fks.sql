-- store/migrations/013_fix_aggregate_metadata_fks.sql
-- Replace the whole file with this (column add BEFORE constraint add):

ALTER TABLE orders
    ADD CONSTRAINT orders_branch_id_fkey
        FOREIGN KEY (branch_id) REFERENCES branches(id),
    ADD CONSTRAINT orders_customer_id_fkey
        FOREIGN KEY (customer_id) REFERENCES customers(id);

-- Add column FIRST, then constraint
ALTER TABLE supply_requests
    ADD COLUMN IF NOT EXISTS supplier_id UUID;

ALTER TABLE supply_requests
    ADD CONSTRAINT supply_requests_branch_id_fkey
        FOREIGN KEY (branch_id) REFERENCES branches(id),
    ADD CONSTRAINT supply_requests_supplier_id_fkey
        FOREIGN KEY (supplier_id) REFERENCES suppliers(id);

CREATE INDEX IF NOT EXISTS idx_orders_branch ON orders(branch_id);
CREATE INDEX IF NOT EXISTS idx_orders_customer ON orders(customer_id);
CREATE INDEX IF NOT EXISTS idx_supply_requests_branch ON supply_requests(branch_id);
CREATE INDEX IF NOT EXISTS idx_supply_requests_supplier ON supply_requests(supplier_id);