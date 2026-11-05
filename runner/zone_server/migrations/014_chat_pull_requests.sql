SET LOCAL lock_timeout = '5s';

CREATE TABLE public.chat_pull_requests (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    chat_id uuid NOT NULL,
    provider text DEFAULT 'github'::text NOT NULL,
    owner text NOT NULL,
    repo text NOT NULL,
    number bigint NOT NULL,
    url text NOT NULL,
    title text,
    source_id uuid,
    dismissed_at timestamp with time zone,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT chat_pull_requests_pkey PRIMARY KEY (id),
    CONSTRAINT chat_pull_requests_chat_id_fkey FOREIGN KEY (chat_id) REFERENCES public.chats(id) ON DELETE CASCADE,
    CONSTRAINT chat_pull_requests_source_id_fkey FOREIGN KEY (source_id) REFERENCES public.sources(id) ON DELETE SET NULL,
    CONSTRAINT chat_pull_requests_provider_check CHECK (provider = ANY (ARRAY['github'::text])),
    CONSTRAINT chat_pull_requests_number_check CHECK (number > 0)
);

CREATE UNIQUE INDEX chat_pull_requests_chat_pull_request_key
    ON public.chat_pull_requests USING btree (chat_id, provider, lower(owner), lower(repo), number);

CREATE INDEX chat_pull_requests_source_id_idx
    ON public.chat_pull_requests USING btree (source_id);
