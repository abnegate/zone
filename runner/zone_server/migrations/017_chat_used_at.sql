SET LOCAL lock_timeout = '5s';

ALTER TABLE public.chats ADD COLUMN used_at timestamp without time zone DEFAULT now() NOT NULL;

UPDATE public.chats SET used_at = COALESCE(updated_at, created_at, used_at);

CREATE INDEX idx_chats_workspace_used
    ON public.chats USING btree (workspace_id, used_at DESC)
    WHERE purpose = 'assistant' AND archived IS NOT TRUE;
