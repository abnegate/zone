SET LOCAL lock_timeout = '5s';

ALTER TABLE public.chats ADD COLUMN used_at timestamp without time zone DEFAULT now() NOT NULL;

UPDATE public.chats AS chat
SET used_at = COALESCE(GREATEST(chat.created_at, (
    SELECT max(message.created_at)
    FROM public.messages AS message
    WHERE message.chat_id = chat.id
        AND message.role = 'user'
        AND message.metadata -> 'source' IS NULL
)), chat.used_at);

CREATE INDEX idx_chats_workspace_used
    ON public.chats USING btree (workspace_id, used_at DESC)
    WHERE purpose = 'assistant' AND archived IS NOT TRUE;
