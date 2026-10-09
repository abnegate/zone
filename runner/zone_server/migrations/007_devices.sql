SET LOCAL lock_timeout = '5s';

CREATE TABLE public.devices (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    user_id uuid NOT NULL,
    public_id uuid NOT NULL,
    name text,
    platform text NOT NULL,
    user_agent text,
    last_ip inet,
    last_seen_at timestamp with time zone DEFAULT now() NOT NULL,
    status text DEFAULT 'allowed'::text NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT devices_pkey PRIMARY KEY (id),
    CONSTRAINT devices_user_id_fkey FOREIGN KEY (user_id) REFERENCES public.users(id) ON DELETE CASCADE,
    CONSTRAINT devices_user_public_id_key UNIQUE (user_id, public_id),
    CONSTRAINT devices_platform_check CHECK ((platform = ANY (ARRAY['android'::text, 'ios'::text, 'desktop'::text, 'browser'::text, 'cli'::text]))),
    CONSTRAINT devices_status_check CHECK ((status = ANY (ARRAY['allowed'::text, 'pending'::text, 'blocked'::text]))),
    CONSTRAINT devices_name_check CHECK (((name IS NULL) OR (length(TRIM(BOTH FROM name)) > 0)))
);

CREATE INDEX devices_user_id_idx ON public.devices USING btree (user_id);
CREATE INDEX devices_status_idx ON public.devices USING btree (status);
CREATE INDEX devices_last_seen_at_idx ON public.devices USING btree (last_seen_at DESC);

ALTER TABLE public.sessions ADD COLUMN device_id uuid;

DO $$
DECLARE
    rec RECORD;
    new_id uuid;
BEGIN
    FOR rec IN
        SELECT id, user_id, user_agent, ip_address, last_active_at, created_at
        FROM public.sessions
        WHERE revoked_at IS NULL AND expires_at > NOW()
    LOOP
        INSERT INTO public.devices (
            user_id,
            public_id,
            platform,
            user_agent,
            last_ip,
            last_seen_at,
            status,
            created_at,
            updated_at
        )
        VALUES (
            rec.user_id,
            gen_random_uuid(),
            'browser',
            rec.user_agent,
            rec.ip_address,
            rec.last_active_at,
            'allowed',
            rec.created_at,
            NOW()
        )
        RETURNING id INTO new_id;

        UPDATE public.sessions
        SET device_id = new_id
        WHERE id = rec.id;
    END LOOP;
END $$;

ALTER TABLE public.sessions
    ADD CONSTRAINT sessions_device_id_fkey FOREIGN KEY (device_id) REFERENCES public.devices(id);

CREATE INDEX sessions_device_id_idx ON public.sessions USING btree (device_id);

CREATE TABLE public.device_policy (
    id boolean DEFAULT true NOT NULL,
    mode text DEFAULT 'open'::text NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT device_policy_pkey PRIMARY KEY (id),
    CONSTRAINT device_policy_singleton CHECK (id),
    CONSTRAINT device_policy_mode_check CHECK ((mode = ANY (ARRAY['open'::text, 'allowed'::text])))
);

INSERT INTO public.device_policy (id, mode) VALUES (true, 'open');
