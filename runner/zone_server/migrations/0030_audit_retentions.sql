SET LOCAL lock_timeout = '5s';

CREATE TABLE public.audit_retentions (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    organization_id uuid,
    days integer NOT NULL,
    updated_by uuid,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    swept_at timestamp with time zone,
    deleted_count bigint DEFAULT 0 NOT NULL,
    CONSTRAINT audit_retentions_pkey PRIMARY KEY (id),
    CONSTRAINT audit_retentions_days_check CHECK (days > 0),
    CONSTRAINT audit_retentions_organization_id_fkey FOREIGN KEY (organization_id) REFERENCES public.organizations(id) ON DELETE CASCADE,
    CONSTRAINT audit_retentions_updated_by_fkey FOREIGN KEY (updated_by) REFERENCES public.users(id) ON DELETE SET NULL
);

CREATE UNIQUE INDEX audit_retentions_scope_idx ON public.audit_retentions USING btree ((COALESCE(organization_id, '00000000-0000-0000-0000-000000000000'::uuid)));
