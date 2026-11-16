SET LOCAL lock_timeout = '5s';

CREATE TABLE public.licenses (
    id smallint DEFAULT 1 NOT NULL,
    key text NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT licenses_pkey PRIMARY KEY (id),
    CONSTRAINT licenses_singleton_check CHECK (id = 1)
);
