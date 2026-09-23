SET LOCAL lock_timeout = '5s';

ALTER TABLE organization_ai_settings DROP CONSTRAINT IF EXISTS organization_ai_settings_provider_check;
ALTER TABLE organization_ai_settings ADD CONSTRAINT organization_ai_settings_provider_check
    CHECK (provider IN ('self_hosted', 'openai', 'anthropic', 'bedrock', 'claude_code', 'codex'));

ALTER TABLE workspace_ai_settings DROP CONSTRAINT IF EXISTS workspace_ai_settings_provider_check;
ALTER TABLE workspace_ai_settings ADD CONSTRAINT workspace_ai_settings_provider_check
    CHECK (provider IS NULL OR provider IN ('self_hosted', 'openai', 'anthropic', 'bedrock', 'claude_code', 'codex'));

CREATE TABLE agent_logins (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    organization_id UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    agent TEXT NOT NULL CHECK (agent IN ('claude', 'codex')),
    credential TEXT,
    label TEXT,
    expires_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (organization_id, agent)
);
