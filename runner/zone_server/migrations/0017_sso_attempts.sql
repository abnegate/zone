SET LOCAL lock_timeout = '5s';

CREATE TABLE public.sso_attempts (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    state_hash text NOT NULL,
    provider_id uuid NOT NULL,
    nonce text NOT NULL,
    code_verifier text NOT NULL,
    expires_at timestamp with time zone NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT sso_attempts_pkey PRIMARY KEY (id),
    CONSTRAINT sso_attempts_state_hash_key UNIQUE (state_hash),
    CONSTRAINT sso_attempts_provider_id_fkey FOREIGN KEY (provider_id) REFERENCES public.sso_providers(id) ON DELETE CASCADE
);
CREATE INDEX sso_attempts_expires_at_idx ON public.sso_attempts USING btree (expires_at);
