-- S06 fix: branch_id and actor_id are unknown at registration time.
-- Owner assigns them at confirm time via the dashboard.
ALTER TABLE actor_directory
    ALTER COLUMN branch_id DROP NOT NULL,
    ALTER COLUMN actor_id DROP NOT NULL;
