SET LOCAL lock_timeout = '5s';

CREATE TABLE public.sso_identities (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    provider_id uuid NOT NULL,
    user_id uuid NOT NULL,
    subject text NOT NULL,
    email text NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    last_signed_in_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT sso_identities_pkey PRIMARY KEY (id),
    CONSTRAINT sso_identities_provider_id_subject_key UNIQUE (provider_id, subject),
    CONSTRAINT sso_identities_provider_id_user_id_key UNIQUE (provider_id, user_id),
    CONSTRAINT sso_identities_provider_id_fkey FOREIGN KEY (provider_id) REFERENCES public.sso_providers(id) ON DELETE CASCADE,
    CONSTRAINT sso_identities_user_id_fkey FOREIGN KEY (user_id) REFERENCES public.users(id) ON DELETE CASCADE
);
CREATE INDEX sso_identities_user_id_idx ON public.sso_identities USING btree (user_id);
