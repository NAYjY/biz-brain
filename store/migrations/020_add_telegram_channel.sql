ALTER TABLE webhook_inbox
    DROP CONSTRAINT webhook_inbox_channel_check,
    ADD CONSTRAINT webhook_inbox_channel_check
        CHECK (channel IN ('line', 'whats_app', 'telegram'));

ALTER TABLE actor_directory
    DROP CONSTRAINT actor_directory_channel_check,
    ADD CONSTRAINT actor_directory_channel_check
        CHECK (channel IN ('line', 'whats_app', 'telegram'));