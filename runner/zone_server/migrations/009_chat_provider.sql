SET LOCAL lock_timeout = '5s';

ALTER TABLE public.chats
    ADD COLUMN provider text,
    ADD COLUMN provider_model text,
    ADD CONSTRAINT chats_provider_check CHECK (
        provider IS NULL
        OR provider = ANY (ARRAY['self_hosted'::text, 'openai'::text, 'anthropic'::text, 'bedrock'::text, 'claude_code'::text, 'codex'::text])
    ) NOT VALID,
    ADD CONSTRAINT chats_provider_model_check CHECK (
        provider_model IS NULL OR (provider IS NOT NULL AND btrim(provider_model) <> '')
    ) NOT VALID,
    DROP CONSTRAINT chats_reasoning_effort_check,
    ADD CONSTRAINT chats_reasoning_effort_check CHECK (
        reasoning_effort = ANY (ARRAY['auto'::text, 'off'::text, 'low'::text, 'medium'::text, 'high'::text, 'xhigh'::text, 'max'::text, 'ultra'::text])
    ) NOT VALID;
