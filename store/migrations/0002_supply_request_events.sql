-- T02: supply_request_events — per-aggregate-instance stream, Option B schema.

CREATE TABLE supply_request_events (
    aggregate_id    UUID NOT NULL,
    sequence        BIGINT NOT NULL,
    branch_id       UUID NOT NULL,
    event_type      VARCHAR(50) NOT NULL,
    occurred_at     TIMESTAMPTZ NOT NULL DEFAULT NOW(),

    supplier_id     UUID,
    invoice_id      UUID,

    PRIMARY KEY (aggregate_id, sequence),

    CONSTRAINT supply_request_events_type_check CHECK (
        event_type IN (
            'supply_request_sent', 'invoice_received',
            'invoice_approved', 'supplier_confirmed'
        )
    ),

    CONSTRAINT chk_sre_supplier_id CHECK (
        event_type = 'invoice_approved' OR supplier_id IS NOT NULL
    ),
    CONSTRAINT chk_sre_invoice_id CHECK (
        event_type = 'supply_request_sent' OR invoice_id IS NOT NULL
    )
);

CREATE INDEX idx_supply_request_events_aggregate ON supply_request_events (aggregate_id, sequence);
