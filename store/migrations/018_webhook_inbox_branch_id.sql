-- S06 follow-up: add branch_id to webhook_inbox so inbox_worker can
-- call actor_directory.register_pending() for unknown senders without
-- a separate lookup join.
--
-- Nullable because existing rows pre-date this column; new rows
-- populated by messaging::inbound::receive at insertion time.

ALTER TABLE webhook_inbox
    ADD COLUMN branch_id UUID REFERENCES branches(id);
