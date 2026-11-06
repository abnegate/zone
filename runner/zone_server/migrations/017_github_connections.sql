SET LOCAL lock_timeout = '5s';

CREATE TABLE public.github_apps (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    organization_id uuid NOT NULL,
    app_id bigint NOT NULL,
    slug text NOT NULL,
    name text NOT NULL,
    owner_login text NOT NULL,
    owner_type text NOT NULL,
    html_url text NOT NULL,
    client_id text NOT NULL,
    client_secret text NOT NULL,
    private_key text NOT NULL,
    webhook_secret text,
    created_by uuid,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT github_apps_pkey PRIMARY KEY (id),
    CONSTRAINT github_apps_owner_type_check CHECK ((owner_type = ANY (ARRAY['User'::text, 'Organization'::text]))),
    CONSTRAINT github_apps_organization_id_fkey FOREIGN KEY (organization_id) REFERENCES public.organizations(id) ON DELETE CASCADE,
    CONSTRAINT github_apps_created_by_fkey FOREIGN KEY (created_by) REFERENCES public.users(id) ON DELETE SET NULL
);

CREATE UNIQUE INDEX github_apps_organization_owner_idx
    ON public.github_apps USING btree (organization_id, lower(owner_login));

CREATE TABLE public.github_installations (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    organization_id uuid NOT NULL,
    github_app_id uuid NOT NULL,
    installation_id bigint NOT NULL,
    account_login text NOT NULL,
    account_type text NOT NULL,
    repository_selection text NOT NULL,
    html_url text NOT NULL,
    suspended_at timestamp with time zone,
    revoked_at timestamp with time zone,
    created_by uuid,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT github_installations_pkey PRIMARY KEY (id),
    CONSTRAINT github_installations_app_installation_key UNIQUE (github_app_id, installation_id),
    CONSTRAINT github_installations_account_type_check CHECK ((account_type = ANY (ARRAY['User'::text, 'Organization'::text, 'Enterprise'::text]))),
    CONSTRAINT github_installations_repository_selection_check CHECK ((repository_selection = ANY (ARRAY['all'::text, 'selected'::text]))),
    CONSTRAINT github_installations_organization_id_fkey FOREIGN KEY (organization_id) REFERENCES public.organizations(id) ON DELETE CASCADE,
    CONSTRAINT github_installations_github_app_id_fkey FOREIGN KEY (github_app_id) REFERENCES public.github_apps(id) ON DELETE CASCADE,
    CONSTRAINT github_installations_created_by_fkey FOREIGN KEY (created_by) REFERENCES public.users(id) ON DELETE SET NULL
);

CREATE INDEX github_installations_organization_id_idx
    ON public.github_installations USING btree (organization_id);

CREATE TABLE public.github_connection_states (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    nonce_hash text NOT NULL,
    organization_id uuid NOT NULL,
    user_id uuid NOT NULL,
    github_app_id uuid,
    purpose text NOT NULL,
    expires_at timestamp with time zone NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT github_connection_states_pkey PRIMARY KEY (id),
    CONSTRAINT github_connection_states_nonce_hash_key UNIQUE (nonce_hash),
    CONSTRAINT github_connection_states_purpose_check CHECK ((purpose = ANY (ARRAY['registration'::text, 'installation'::text]))),
    CONSTRAINT github_connection_states_organization_id_fkey FOREIGN KEY (organization_id) REFERENCES public.organizations(id) ON DELETE CASCADE,
    CONSTRAINT github_connection_states_user_id_fkey FOREIGN KEY (user_id) REFERENCES public.users(id) ON DELETE CASCADE,
    CONSTRAINT github_connection_states_github_app_id_fkey FOREIGN KEY (github_app_id) REFERENCES public.github_apps(id) ON DELETE CASCADE
);

CREATE INDEX github_connection_states_expires_at_idx
    ON public.github_connection_states USING btree (expires_at);

ALTER TABLE public.sources
    ADD COLUMN github_installation_id uuid,
    ADD CONSTRAINT sources_github_installation_id_fkey FOREIGN KEY (github_installation_id) REFERENCES public.github_installations(id) ON DELETE SET NULL;

CREATE INDEX sources_github_installation_id_idx
    ON public.sources USING btree (github_installation_id)
    WHERE github_installation_id IS NOT NULL;

ALTER TABLE public.projects
    ADD COLUMN github_installation_id uuid,
    ADD CONSTRAINT projects_github_installation_id_fkey FOREIGN KEY (github_installation_id) REFERENCES public.github_installations(id) ON DELETE SET NULL;

CREATE INDEX projects_github_installation_id_idx
    ON public.projects USING btree (github_installation_id)
    WHERE github_installation_id IS NOT NULL;

UPDATE public.sources SET config = config - 'github_app' WHERE config ? 'github_app';
