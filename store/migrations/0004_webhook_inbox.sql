-- T04: durable raw-payload landing table for LINE/WhatsApp webhooks.
-- Ack the webhook fast (R01/R02), persist raw payload, process async.
-- Dedup lives here (pre-domain), not on the DomainEvent tables.

CREATE TABLE webhook_inbox (
    id                  BIGSERIAL PRIMARY KEY,
    channel             VARCHAR(20) NOT NULL CHECK (channel IN ('line', 'whats_app')),
    external_event_id   VARCHAR(255) NOT NULL,
    raw_payload         JSONB NOT NULL,
    received_at         TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    processed_at        TIMESTAMPTZ,

    CONSTRAINT uq_webhook_inbox_dedup UNIQUE (channel, external_event_id)
);

CREATE INDEX idx_webhook_inbox_unprocessed ON webhook_inbox (received_at) WHERE processed_at IS NULL;
