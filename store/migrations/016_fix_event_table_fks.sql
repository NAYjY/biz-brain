-- 0001/0002 created event tables without REFERENCES (orders/supply_requests
-- didn't have proper PKs with FKs yet). Wire up the FKs now.

ALTER TABLE order_events
    ADD CONSTRAINT oe_aggregate_id_fkey
        FOREIGN KEY (aggregate_id) REFERENCES orders(id),
    ADD CONSTRAINT oe_branch_id_fkey
        FOREIGN KEY (branch_id) REFERENCES branches(id);

ALTER TABLE supply_request_events
    ADD CONSTRAINT sre_aggregate_id_fkey
        FOREIGN KEY (aggregate_id) REFERENCES supply_requests(id),
    ADD CONSTRAINT sre_branch_id_fkey
        FOREIGN KEY (branch_id) REFERENCES branches(id);
