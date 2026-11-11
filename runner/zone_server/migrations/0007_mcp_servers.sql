SET LOCAL lock_timeout = '5s';

CREATE TABLE public.mcp_servers (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    organization_id uuid NOT NULL,
    name text NOT NULL,
    transport text NOT NULL,
    command text,
    arguments text[] DEFAULT '{}'::text[] NOT NULL,
    url text,
    tools text[],
    enabled boolean DEFAULT true NOT NULL,
    secrets text,
    created_by uuid,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT mcp_servers_pkey PRIMARY KEY (id),
    CONSTRAINT mcp_servers_id_organization_key UNIQUE (id, organization_id),
    CONSTRAINT mcp_servers_organization_name_key UNIQUE (organization_id, name),
    CONSTRAINT mcp_servers_transport_check CHECK ((transport = ANY (ARRAY['stdio'::text, 'http'::text]))),
    CONSTRAINT mcp_servers_target_check CHECK (((transport = 'stdio' AND command IS NOT NULL AND url IS NULL) OR (transport <> 'stdio' AND url IS NOT NULL AND command IS NULL))),
    CONSTRAINT mcp_servers_name_check CHECK ((name ~ '^[A-Za-z0-9_-]{1,32}$' AND position('__' in name) = 0 AND name !~ '_$')),
    CONSTRAINT mcp_servers_organization_id_fkey FOREIGN KEY (organization_id) REFERENCES public.organizations(id) ON DELETE CASCADE,
    CONSTRAINT mcp_servers_created_by_fkey FOREIGN KEY (created_by) REFERENCES public.users(id) ON DELETE SET NULL
);

CREATE TABLE public.workspace_mcp_servers (
    workspace_id uuid NOT NULL,
    server_id uuid NOT NULL,
    organization_id uuid NOT NULL,
    enabled boolean NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT workspace_mcp_servers_pkey PRIMARY KEY (workspace_id, server_id),
    CONSTRAINT workspace_mcp_servers_workspace_id_fkey FOREIGN KEY (workspace_id) REFERENCES public.workspaces(id) ON DELETE CASCADE,
    CONSTRAINT workspace_mcp_servers_server_organization_fkey FOREIGN KEY (server_id, organization_id) REFERENCES public.mcp_servers(id, organization_id) ON DELETE CASCADE
);

CREATE INDEX workspace_mcp_servers_server_idx ON public.workspace_mcp_servers USING btree (server_id);
