ALTER TABLE public.chats
    ADD COLUMN context_tokens bigint,
    ADD CONSTRAINT chats_context_tokens_check CHECK (context_tokens IS NULL OR context_tokens > 0);
