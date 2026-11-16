SET LOCAL lock_timeout = '5s';

CREATE TABLE public.sso_settings (
    id boolean DEFAULT true NOT NULL,
    required boolean DEFAULT false NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT sso_settings_pkey PRIMARY KEY (id),
    CONSTRAINT sso_settings_singleton_check CHECK (id)
);
