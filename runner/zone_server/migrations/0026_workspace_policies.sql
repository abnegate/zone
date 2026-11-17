SET LOCAL lock_timeout = '5s';

CREATE TABLE public.workspace_policies (
    workspace_id uuid NOT NULL,
    rules jsonb DEFAULT '{}'::jsonb NOT NULL,
    updated_by uuid,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT workspace_policies_pkey PRIMARY KEY (workspace_id),
    CONSTRAINT workspace_policies_rules_check CHECK (jsonb_typeof(rules) = 'object'),
    CONSTRAINT workspace_policies_workspace_id_fkey FOREIGN KEY (workspace_id) REFERENCES public.workspaces(id) ON DELETE CASCADE,
    CONSTRAINT workspace_policies_updated_by_fkey FOREIGN KEY (updated_by) REFERENCES public.users(id) ON DELETE SET NULL
);
