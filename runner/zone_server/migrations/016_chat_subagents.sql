SET LOCAL lock_timeout = '5s';

CREATE TABLE public.chat_subagents (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    chat_id uuid NOT NULL,
    message_id uuid,
    external_id text NOT NULL,
    description text DEFAULT ''::text NOT NULL,
    kind text,
    model text,
    background boolean DEFAULT false NOT NULL,
    status text DEFAULT 'running'::text NOT NULL,
    tool_uses bigint,
    duration_ms bigint,
    tokens bigint,
    last_tool text,
    summary text,
    error text,
    steps jsonb DEFAULT '[]'::jsonb NOT NULL,
    started_at timestamp with time zone DEFAULT now() NOT NULL,
    finished_at timestamp with time zone,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT chat_subagents_pkey PRIMARY KEY (id),
    CONSTRAINT chat_subagents_chat_id_fkey FOREIGN KEY (chat_id) REFERENCES public.chats(id) ON DELETE CASCADE,
    CONSTRAINT chat_subagents_status_check CHECK (status = ANY (ARRAY['running'::text, 'completed'::text, 'failed'::text, 'stopped'::text])),
    CONSTRAINT chat_subagents_steps_check CHECK (jsonb_typeof(steps) = 'array')
);

CREATE UNIQUE INDEX chat_subagents_chat_external_key
    ON public.chat_subagents USING btree (chat_id, external_id);

CREATE INDEX chat_subagents_chat_started_idx
    ON public.chat_subagents USING btree (chat_id, started_at);
