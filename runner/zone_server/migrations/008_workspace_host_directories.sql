SET LOCAL lock_timeout = '5s';

CREATE TABLE public.workspace_host_directories (
    workspace_id uuid NOT NULL,
    directories text[] DEFAULT '{}'::text[] NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT workspace_host_directories_pkey PRIMARY KEY (workspace_id),
    CONSTRAINT workspace_host_directories_workspace_id_fkey
        FOREIGN KEY (workspace_id) REFERENCES public.workspaces(id) ON DELETE CASCADE
);
