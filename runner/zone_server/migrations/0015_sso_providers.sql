SET LOCAL lock_timeout = '5s';

CREATE TABLE public.sso_providers (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    protocol text DEFAULT 'oidc' NOT NULL,
    display_name text NOT NULL,
    issuer text NOT NULL,
    client_id text NOT NULL,
    client_secret text NOT NULL,
    allowed_domains text[] DEFAULT '{}'::text[] NOT NULL,
    require_verified_email boolean DEFAULT true NOT NULL,
    enabled boolean DEFAULT true NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT sso_providers_pkey PRIMARY KEY (id),
    CONSTRAINT sso_providers_protocol_check CHECK (protocol = 'oidc'),
    CONSTRAINT sso_providers_issuer_client_id_key UNIQUE (issuer, client_id)
);
