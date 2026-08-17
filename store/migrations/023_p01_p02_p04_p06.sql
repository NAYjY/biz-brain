-- P01: Branch-scoped reply templates (Owner-editable, seeded on Branch creation).
CREATE TABLE reply_templates (
    id          UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    branch_id   UUID        NOT NULL REFERENCES branches(id) ON DELETE CASCADE,
    event_type  VARCHAR(64) NOT NULL,
    template    TEXT        NOT NULL,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (branch_id, event_type)
);
CREATE INDEX idx_reply_templates_branch ON reply_templates(branch_id);

-- P13: Per-sender conversation history (keyed on sender identity string).
-- Sliding window of 20 messages, oldest pruned on insert via trigger or
-- application logic.  role IN ('user','assistant').
CREATE TABLE conversation_history (
    id          BIGSERIAL   PRIMARY KEY,
    sender_key  VARCHAR(512) NOT NULL,   -- channel:external_id
    role        VARCHAR(16)  NOT NULL CHECK (role IN ('user', 'assistant')),
    content     TEXT         NOT NULL,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX idx_conv_history_sender ON conversation_history(sender_key, id DESC);

-- P02: Disambiguation state.  One row per pending sender thread.
-- aggregate_type added for P06 (SupplierConfirmed can also need it).
CREATE TABLE disambiguation_pending (
    id              UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    sender_key      VARCHAR(512) NOT NULL UNIQUE,
    original_text   TEXT        NOT NULL,
    candidate_ids   JSONB       NOT NULL,   -- ordered list of UUID strings
    candidate_index INTEGER     NOT NULL DEFAULT 0,
    aggregate_type  VARCHAR(20) NOT NULL DEFAULT 'order'
                    CHECK (aggregate_type IN ('order', 'supply_request')),
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX idx_disambig_sender ON disambiguation_pending(sender_key);

-- P04: Extend order_events CHECK to include new event types.
ALTER TABLE order_events
    DROP CONSTRAINT order_events_type_check,
    ADD CONSTRAINT order_events_type_check CHECK (
        event_type IN (
            'worker_assigned', 'worker_accepted', 'worker_unavailable',
            'worker_cancelled', 'clarification_requested',
            'worker_ready_for_pickup', 'order_done',
            'owner_cancelled', 'order_reset', 'clarification_resolved'
        )
    );

-- P04: worker_id nullable for order_reset and owner_cancelled (no worker for reset).
-- Relax existing NOT NULL constraint for order_done, add new exception rows.
ALTER TABLE order_events
    DROP CONSTRAINT IF EXISTS chk_order_events_worker_id,
    ADD CONSTRAINT chk_order_events_worker_id CHECK (
        event_type IN ('order_done', 'order_reset') OR worker_id IS NOT NULL
    );

-- P05: Store fetched invoice media as bytea on invoices table.
ALTER TABLE invoices
    ADD COLUMN IF NOT EXISTS media_data      BYTEA,
    ADD COLUMN IF NOT EXISTS media_mime_type VARCHAR(128);
