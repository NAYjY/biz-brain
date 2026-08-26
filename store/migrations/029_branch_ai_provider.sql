-- T01 resolution + per-branch AI provider config.
-- Branches can independently choose between 'claude' (default) and 'gemini'.
-- New branches default to 'claude' so existing behaviour is unchanged.

ALTER TABLE branches
    ADD COLUMN IF NOT EXISTS ai_provider VARCHAR(16) NOT NULL DEFAULT 'claude'
        CHECK (ai_provider IN ('claude', 'gemini'));