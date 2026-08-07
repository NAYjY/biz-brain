-- T01: Invoice is a separate aggregate referencing SupplyRequestId.
-- D05: invoice_current_state projection added in same ticket.

CREATE TABLE invoices (
    id                  UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    supply_request_id   UUID        NOT NULL REFERENCES supply_requests(id),
    branch_id           UUID        NOT NULL REFERENCES branches(id),
    supplier_id         UUID        NOT NULL REFERENCES suppliers(id),
    notes               TEXT,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_invoices_supply_request ON invoices(supply_request_id);
CREATE INDEX idx_invoices_branch ON invoices(branch_id);
