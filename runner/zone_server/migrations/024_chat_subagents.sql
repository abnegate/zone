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
    revision bigint DEFAULT 0 NOT NULL,
    started_at timestamp with time zone DEFAULT now() NOT NULL,
    finished_at timestamp with time zone,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT chat_subagents_pkey PRIMARY KEY (id),
    CONSTRAINT chat_subagents_chat_id_fkey FOREIGN KEY (chat_id) REFERENCES public.chats(id) ON DELETE CASCADE,
    CONSTRAINT chat_subagents_status_check CHECK (status = ANY (ARRAY['running'::text, 'completed'::text, 'failed'::text, 'stopped'::text]))
);

CREATE UNIQUE INDEX chat_subagents_chat_external_key
    ON public.chat_subagents USING btree (chat_id, external_id);

CREATE INDEX chat_subagents_chat_started_idx
    ON public.chat_subagents USING btree (chat_id, started_at);

CREATE INDEX chat_subagents_running_idx
    ON public.chat_subagents USING btree (chat_id, message_id)
    WHERE status = 'running';

CREATE TABLE public.chat_subagent_steps (
    subagent_id uuid NOT NULL,
    step_id text NOT NULL,
    "position" bigint NOT NULL,
    name text NOT NULL,
    subject text,
    status text DEFAULT 'running'::text NOT NULL,
    CONSTRAINT chat_subagent_steps_pkey PRIMARY KEY (subagent_id, step_id),
    CONSTRAINT chat_subagent_steps_subagent_id_fkey FOREIGN KEY (subagent_id) REFERENCES public.chat_subagents(id) ON DELETE CASCADE,
    CONSTRAINT chat_subagent_steps_status_check CHECK (status = ANY (ARRAY['running'::text, 'succeeded'::text, 'failed'::text]))
);

CREATE INDEX chat_subagent_steps_order_idx
    ON public.chat_subagent_steps USING btree (subagent_id, "position");
