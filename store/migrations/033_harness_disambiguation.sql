-- 033: Harness disambiguation — replaces the thin yes/no loop state with
-- a rich structure the Gemini harness can read and write across multiple turns.
--
-- Key changes from the original disambiguation_pending:
--   DROP candidate_index  — sequential yes/no loop is gone
--   ADD turns_elapsed     — harness tracks how many times it has asked
--   ADD extracted_notes   — JSONB array, accumulated across turns before declaration
--   ADD last_question     — harness rephrases each turn, never repeats exactly
--   ADD narrowed_to       — once harness narrows from N candidates to 1, stored here
--   ADD original_intent   — the event variant detected on turn 1 (e.g. "order_done")
--                           carried forward so continuation knows what to emit on declaration
--   ADD escalated_at      — set when turns_elapsed reaches 3, owner is notified
--   ADD created_at        — for TTL cleanup (stale rows older than 24h should be purged)
--
-- Escalation → owner_alert:
--   When escalated_at is set, inbox_worker sets ai_routed_low_confidence = TRUE
--   on the relevant order_current_state row so the ⚠️ badge appears on dashboard.
--   Owner clicks thread to see the full accumulated conversation + notes.

-- Drop old table and recreate clean.
-- Safe: the yes/no loop is being fully replaced. Any in-flight disambiguation
-- rows from the old schema are stale anyway (they require the old candidate_index
-- logic to resolve, which will no longer exist after this deploy).
DROP TABLE IF EXISTS disambiguation_pending;

CREATE TABLE disambiguation_pending (
    id              UUID        PRIMARY KEY DEFAULT gen_random_uuid(),

    -- Who is in this flow
    sender_key      VARCHAR(512) NOT NULL UNIQUE,   -- channel:external_id

    -- What they originally said (turn 1 message)
    original_text   TEXT        NOT NULL,

    -- Which orders are still candidates (ordered JSON array of UUID strings)
    candidate_ids   JSONB       NOT NULL,

    -- Aggregate type: 'order' or 'supply_request'
    aggregate_type  VARCHAR(20) NOT NULL DEFAULT 'order'
                    CHECK (aggregate_type IN ('order', 'supply_request')),

    -- What event the harness detected on turn 1
    -- Carried forward so continuation knows what to emit when declaration arrives.
    -- e.g. "order_done", "worker_cancelled", "clarification_requested"
    original_intent VARCHAR(50),

    -- Harness turn tracking
    turns_elapsed   INTEGER     NOT NULL DEFAULT 0,

    -- Notes extracted across all turns before declaration
    -- e.g. ["น้ำยาหมด", "ลูกค้าร้องเรื่องเสียงดัง"]
    extracted_notes JSONB       NOT NULL DEFAULT '[]',

    -- Last question asked — harness reads this to rephrase next turn
    last_question   TEXT,

    -- Once harness narrows from N candidates to 1, stored here.
    -- Continuation then asks confirmation for this specific order.
    narrowed_to     UUID,

    -- Set when turns_elapsed reaches 3 — owner gets ⚠️ on dashboard
    escalated_at    TIMESTAMPTZ,

    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Hot-path lookup: inbox_worker checks this on every inbound message
CREATE INDEX idx_disambig_sender ON disambiguation_pending(sender_key);

-- TTL cleanup: background job can purge rows older than 24h
-- (worker clearly moved on, stale state should not affect new messages)
CREATE INDEX idx_disambig_created ON disambiguation_pending(created_at)
    WHERE escalated_at IS NULL;