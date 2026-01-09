-- AI Provider Settings
-- Supports self-hosted (Ollama via LiteLLM), OpenAI, Anthropic, and AWS Bedrock

-- Organization-level AI settings (defaults/fallback)
CREATE TABLE organization_ai_settings (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    organization_id UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE UNIQUE,

    -- Provider selection
    provider TEXT NOT NULL DEFAULT 'self_hosted'
        CHECK (provider IN ('self_hosted', 'openai', 'anthropic', 'bedrock')),

    -- Self-hosted settings (Ollama via LiteLLM)
    litellm_host TEXT,
    litellm_key TEXT,

    -- OpenAI settings
    openai_api_key TEXT,
    openai_base_url TEXT,

    -- Anthropic settings
    anthropic_api_key TEXT,
    anthropic_base_url TEXT,

    -- AWS Bedrock settings
    bedrock_region TEXT,
    bedrock_access_key TEXT,
    bedrock_secret_key TEXT,
    bedrock_use_iam_role BOOLEAN DEFAULT FALSE,

    -- Default model selections per purpose
    model_fast TEXT,
    model_reasoning TEXT,
    model_embedding TEXT,

    created_at TIMESTAMP DEFAULT NOW(),
    updated_at TIMESTAMP DEFAULT NOW()
);

CREATE INDEX idx_org_ai_settings_org_id ON organization_ai_settings(organization_id);

-- Workspace-level AI settings (overrides org settings, NULL = inherit)
CREATE TABLE workspace_ai_settings (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    workspace_id UUID NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE UNIQUE,

    -- Provider selection (NULL = inherit from org)
    provider TEXT CHECK (provider IS NULL OR provider IN ('self_hosted', 'openai', 'anthropic', 'bedrock')),

    -- Self-hosted settings
    litellm_host TEXT,
    litellm_key TEXT,

    -- OpenAI settings
    openai_api_key TEXT,
    openai_base_url TEXT,

    -- Anthropic settings
    anthropic_api_key TEXT,
    anthropic_base_url TEXT,

    -- AWS Bedrock settings
    bedrock_region TEXT,
    bedrock_access_key TEXT,
    bedrock_secret_key TEXT,
    bedrock_use_iam_role BOOLEAN,

    -- Model selections (NULL = inherit from org)
    model_fast TEXT,
    model_reasoning TEXT,
    model_embedding TEXT,

    created_at TIMESTAMP DEFAULT NOW(),
    updated_at TIMESTAMP DEFAULT NOW()
);

CREATE INDEX idx_ws_ai_settings_ws_id ON workspace_ai_settings(workspace_id);
