-- D05: Invoice projection table. Added mid-ticket when the missing
-- Invoice-listing endpoint gap was found. Updated by async projection
-- worker same as order_current_state / supply_request_current_state.

CREATE TABLE invoice_current_state (
    invoice_id          UUID        PRIMARY KEY REFERENCES invoices(id),
    branch_id           UUID        NOT NULL REFERENCES branches(id),
    supply_request_id   UUID        NOT NULL REFERENCES supply_requests(id),
    supplier_id         UUID        NOT NULL REFERENCES suppliers(id),
    state               VARCHAR(32) NOT NULL DEFAULT 'Sent'
                        CHECK (state IN (
                            'Sent',
                            'OwnerApproved',
                            'SupplierConfirmed'
                        )),
    notes               TEXT,
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_ics_branch ON invoice_current_state(branch_id);
CREATE INDEX idx_ics_branch_state ON invoice_current_state(branch_id, state);
