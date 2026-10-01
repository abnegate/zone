SET LOCAL lock_timeout = '5s';
ALTER TABLE organization_ai_settings
    ADD COLUMN completions_routed BOOLEAN NOT NULL DEFAULT false;
ALTER TABLE workspace_ai_settings
    ADD COLUMN completions_routed BOOLEAN NOT NULL DEFAULT false;
