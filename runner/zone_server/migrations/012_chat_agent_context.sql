SET LOCAL lock_timeout = '5s';

CREATE TABLE public.chat_agent_contexts (
    chat_id uuid PRIMARY KEY REFERENCES public.chats(id) ON DELETE CASCADE,
    agent text NOT NULL CHECK (agent = ANY (ARRAY['claude'::text, 'codex'::text])),
    model text NOT NULL,
    window_tokens bigint NOT NULL CHECK (window_tokens > 0),
    used_tokens bigint CHECK (used_tokens IS NULL OR used_tokens > 0),
    updated_at timestamp with time zone DEFAULT now() NOT NULL
);
