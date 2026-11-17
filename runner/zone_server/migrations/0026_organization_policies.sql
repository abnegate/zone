SET LOCAL lock_timeout = '5s';

CREATE TABLE public.organization_policies (
    organization_id uuid NOT NULL,
    rules jsonb DEFAULT '{}'::jsonb NOT NULL,
    updated_by uuid,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT organization_policies_pkey PRIMARY KEY (organization_id),
    CONSTRAINT organization_policies_rules_check CHECK (jsonb_typeof(rules) = 'object'),
    CONSTRAINT organization_policies_organization_id_fkey FOREIGN KEY (organization_id) REFERENCES public.organizations(id) ON DELETE CASCADE,
    CONSTRAINT organization_policies_updated_by_fkey FOREIGN KEY (updated_by) REFERENCES public.users(id) ON DELETE SET NULL
);
