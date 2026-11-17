SET LOCAL lock_timeout = '5s';

CREATE TABLE public.audit_destinations (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    organization_id uuid,
    kind text NOT NULL,
    name text NOT NULL,
    enabled boolean DEFAULT true NOT NULL,
    settings jsonb NOT NULL,
    secrets text NOT NULL,
    created_by uuid,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT audit_destinations_pkey PRIMARY KEY (id),
    CONSTRAINT audit_destinations_kind_check CHECK (kind IN ('webhook', 's3', 'syslog')),
    CONSTRAINT audit_destinations_settings_check CHECK (jsonb_typeof(settings) = 'object'),
    CONSTRAINT audit_destinations_organization_id_fkey FOREIGN KEY (organization_id) REFERENCES public.organizations(id) ON DELETE CASCADE,
    CONSTRAINT audit_destinations_created_by_fkey FOREIGN KEY (created_by) REFERENCES public.users(id) ON DELETE SET NULL
);

CREATE UNIQUE INDEX audit_destinations_name_idx ON public.audit_destinations USING btree ((COALESCE(organization_id, '00000000-0000-0000-0000-000000000000'::uuid)), name);

CREATE INDEX audit_destinations_organization_idx ON public.audit_destinations USING btree (organization_id);
