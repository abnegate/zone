SET LOCAL lock_timeout = '5s';

CREATE TABLE chat_token_usage (
    message_id uuid NOT NULL PRIMARY KEY REFERENCES messages(id) ON DELETE CASCADE,
    chat_id uuid NOT NULL REFERENCES chats(id) ON DELETE CASCADE,
    workspace_id uuid REFERENCES workspaces(id) ON DELETE CASCADE,
    model text NOT NULL,
    reasoning_effort text NOT NULL,
    prompt_tokens bigint,
    completion_tokens bigint,
    total_tokens bigint,
    rounds integer NOT NULL,
    tool_calls integer NOT NULL,
    context_tokens bigint NOT NULL,
    instructions_tokens bigint NOT NULL,
    conversation_tokens bigint NOT NULL,
    tools_tokens bigint NOT NULL,
    results_tokens bigint NOT NULL,
    summary_tokens bigint NOT NULL,
    attachments_tokens bigint,
    overhead_tokens bigint NOT NULL,
    estimated boolean NOT NULL,
    recorded_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT chat_token_usage_reported_check CHECK (
        (prompt_tokens IS NULL AND completion_tokens IS NULL AND total_tokens IS NULL)
        OR (prompt_tokens >= 0 AND completion_tokens >= 0 AND total_tokens >= 0)
    ),
    CONSTRAINT chat_token_usage_rounds_check CHECK (rounds >= 0),
    CONSTRAINT chat_token_usage_tool_calls_check CHECK (tool_calls >= 0),
    CONSTRAINT chat_token_usage_context_check CHECK (
        context_tokens >= 0
        AND instructions_tokens >= 0
        AND conversation_tokens >= 0
        AND tools_tokens >= 0
        AND results_tokens >= 0
        AND summary_tokens >= 0
        AND (attachments_tokens IS NULL OR attachments_tokens >= 0)
        AND overhead_tokens >= 0
    )
);

CREATE INDEX chat_token_usage_chat_idx ON chat_token_usage (chat_id, recorded_at DESC);
CREATE INDEX chat_token_usage_workspace_idx ON chat_token_usage (workspace_id, recorded_at DESC) WHERE workspace_id IS NOT NULL;
CREATE INDEX chat_token_usage_model_idx ON chat_token_usage (workspace_id, model, recorded_at DESC) WHERE workspace_id IS NOT NULL;
