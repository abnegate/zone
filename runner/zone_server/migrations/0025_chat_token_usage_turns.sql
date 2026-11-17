SET LOCAL lock_timeout = '5s';

DROP INDEX chat_token_usage_model_idx;
DROP INDEX chat_token_usage_workspace_idx;

ALTER TABLE chat_token_usage
    ADD COLUMN id uuid DEFAULT gen_random_uuid() NOT NULL,
    ADD COLUMN turn_id uuid;

UPDATE chat_token_usage SET turn_id = message_id;

UPDATE chat_token_usage usage
SET workspace_id = chats.workspace_id
FROM chats
WHERE usage.workspace_id IS NULL AND chats.id = usage.chat_id;

DELETE FROM chat_token_usage WHERE workspace_id IS NULL;

ALTER TABLE chat_token_usage
    DROP CONSTRAINT chat_token_usage_pkey,
    DROP CONSTRAINT chat_token_usage_message_id_fkey,
    DROP CONSTRAINT chat_token_usage_chat_id_fkey,
    DROP CONSTRAINT chat_token_usage_context_check,
    ALTER COLUMN turn_id SET NOT NULL,
    ALTER COLUMN message_id DROP NOT NULL,
    ALTER COLUMN chat_id DROP NOT NULL,
    ALTER COLUMN workspace_id SET NOT NULL,
    ALTER COLUMN context_tokens DROP NOT NULL,
    ALTER COLUMN instructions_tokens DROP NOT NULL,
    ALTER COLUMN conversation_tokens DROP NOT NULL,
    ALTER COLUMN tools_tokens DROP NOT NULL,
    ALTER COLUMN results_tokens DROP NOT NULL,
    ALTER COLUMN summary_tokens DROP NOT NULL,
    ALTER COLUMN overhead_tokens DROP NOT NULL,
    ALTER COLUMN estimated DROP NOT NULL,
    ADD PRIMARY KEY (id),
    ADD CONSTRAINT chat_token_usage_message_id_fkey
        FOREIGN KEY (message_id) REFERENCES messages(id) ON DELETE SET NULL,
    ADD CONSTRAINT chat_token_usage_chat_id_fkey
        FOREIGN KEY (chat_id) REFERENCES chats(id) ON DELETE SET NULL,
    ADD CONSTRAINT chat_token_usage_context_check CHECK (
        (
            context_tokens IS NULL
            AND instructions_tokens IS NULL
            AND conversation_tokens IS NULL
            AND tools_tokens IS NULL
            AND results_tokens IS NULL
            AND summary_tokens IS NULL
            AND attachments_tokens IS NULL
            AND overhead_tokens IS NULL
            AND estimated IS NULL
        )
        OR (
            context_tokens >= 0
            AND instructions_tokens >= 0
            AND conversation_tokens >= 0
            AND tools_tokens >= 0
            AND results_tokens >= 0
            AND summary_tokens >= 0
            AND (attachments_tokens IS NULL OR attachments_tokens >= 0)
            AND overhead_tokens >= 0
            AND estimated IS NOT NULL
        )
    );

CREATE INDEX chat_token_usage_workspace_idx ON chat_token_usage (workspace_id, recorded_at DESC);
CREATE INDEX chat_token_usage_message_idx ON chat_token_usage (message_id);
