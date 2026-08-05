-- T02: order_events — per-aggregate-instance stream, Option B schema.
-- Optimistic concurrency via UNIQUE (aggregate_id, sequence).

CREATE TABLE order_events (
    aggregate_id    UUID NOT NULL,
    sequence        BIGINT NOT NULL,
    branch_id       UUID NOT NULL,
    event_type      VARCHAR(50) NOT NULL,
    occurred_at     TIMESTAMPTZ NOT NULL DEFAULT NOW(),

    -- Typed, nullable-but-CHECK-constrained columns (T02: no JSONB).
    worker_id       UUID,

    PRIMARY KEY (aggregate_id, sequence),

    CONSTRAINT order_events_type_check CHECK (
        event_type IN (
            'worker_assigned', 'worker_accepted', 'worker_unavailable',
            'worker_cancelled', 'clarification_requested',
            'worker_ready_for_pickup', 'order_done'
        )
    ),

    -- worker_id required for every variant except order_done.
    CONSTRAINT chk_order_events_worker_id CHECK (
        event_type = 'order_done' OR worker_id IS NOT NULL
    )
);

CREATE INDEX idx_order_events_aggregate ON order_events (aggregate_id, sequence);
