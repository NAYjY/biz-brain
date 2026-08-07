-- 0005_aggregate_metadata.sql created orders/supply_requests but without
-- REFERENCES constraints (branches/customers/suppliers didn't exist yet).
-- Now that 008/009/006 exist, add the proper FKs.

ALTER TABLE orders
    ADD CONSTRAINT orders_branch_id_fkey
        FOREIGN KEY (branch_id) REFERENCES branches(id),
    ADD CONSTRAINT orders_customer_id_fkey
        FOREIGN KEY (customer_id) REFERENCES customers(id);

ALTER TABLE supply_requests
    ADD CONSTRAINT supply_requests_branch_id_fkey
        FOREIGN KEY (branch_id) REFERENCES branches(id),
    ADD CONSTRAINT supply_requests_supplier_id_fkey
        FOREIGN KEY (supplier_id) REFERENCES suppliers(id);

-- Add supplier_id to supply_requests if missing from 0005
ALTER TABLE supply_requests
    ADD COLUMN IF NOT EXISTS supplier_id UUID REFERENCES suppliers(id);

CREATE INDEX IF NOT EXISTS idx_orders_branch ON orders(branch_id);
CREATE INDEX IF NOT EXISTS idx_orders_customer ON orders(customer_id);
CREATE INDEX IF NOT EXISTS idx_supply_requests_branch ON supply_requests(branch_id);
CREATE INDEX IF NOT EXISTS idx_supply_requests_supplier ON supply_requests(supplier_id);
