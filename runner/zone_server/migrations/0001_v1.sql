CREATE EXTENSION IF NOT EXISTS vector WITH SCHEMA public;

CREATE FUNCTION check_organization_membership(p_user_id uuid, p_organization_id uuid) RETURNS boolean
    LANGUAGE plpgsql
    AS $$
BEGIN
  RETURN EXISTS(SELECT 1 FROM organization_members WHERE user_id = p_user_id AND organization_id = p_organization_id AND is_active = TRUE);
END;
$$;

CREATE FUNCTION check_workspace_membership(p_user_id uuid, p_workspace_id uuid) RETURNS boolean
    LANGUAGE plpgsql
    AS $$
BEGIN
  RETURN EXISTS(SELECT 1 FROM workspace_members WHERE user_id = p_user_id AND workspace_id = p_workspace_id AND is_active = TRUE);
END;
$$;

CREATE FUNCTION claim_next_task(p_worker_id text) RETURNS TABLE(task_id uuid, queue_id uuid)
    LANGUAGE plpgsql
    AS $$
DECLARE
  v_queue_id UUID;
  v_task_id UUID;
BEGIN
  SELECT tq.id, tq.task_id INTO v_queue_id, v_task_id
  FROM task_queue tq
  JOIN tasks t ON t.id = tq.task_id
  WHERE tq.worker_id IS NULL
    AND tq.attempts < tq.max_attempts
    AND t.status IN ('queued', 'created')
  ORDER BY tq.priority DESC, tq.queued_at ASC
  LIMIT 1
  FOR UPDATE SKIP LOCKED;

  IF v_queue_id IS NOT NULL THEN
    UPDATE task_queue
    SET worker_id = p_worker_id, started_at = NOW(), attempts = attempts + 1
    WHERE id = v_queue_id;

    UPDATE tasks
    SET status = 'in_progress', worker_id = p_worker_id, started_at = COALESCE(started_at, NOW()), updated_at = NOW()
    WHERE id = v_task_id;

    RETURN QUERY SELECT v_task_id, v_queue_id;
  END IF;
  RETURN;
END;
$$;

CREATE FUNCTION complete_task_in_queue(p_task_id uuid, p_success boolean) RETURNS void
    LANGUAGE plpgsql
    AS $$
BEGIN
  DELETE FROM task_queue WHERE task_id = p_task_id;

  UPDATE tasks
  SET status = CASE WHEN p_success THEN 'complete' ELSE 'blocked' END,
      worker_id = NULL,
      completed_at = CASE WHEN p_success THEN NOW() ELSE NULL END,
      updated_at = NOW()
  WHERE id = p_task_id;
END;
$$;

CREATE FUNCTION get_organization_role(p_user_id uuid, p_organization_id uuid) RETURNS text
    LANGUAGE plpgsql
    AS $$
DECLARE v_role TEXT;
BEGIN
  SELECT role INTO v_role FROM organization_members WHERE user_id = p_user_id AND organization_id = p_organization_id AND is_active = TRUE;
  RETURN v_role;
END;
$$;

CREATE FUNCTION get_task_source(p_task_id uuid) RETURNS TABLE(source_id uuid, source_type text, config jsonb, credentials text)
    LANGUAGE plpgsql
    AS $$
BEGIN
  RETURN QUERY
  SELECT t.source_id, s.source_type, s.config, s.credentials_encrypted
  FROM tasks t
  LEFT JOIN sources s ON s.id = t.source_id
  WHERE t.id = p_task_id AND s.is_active = TRUE;
END;
$$;

CREATE FUNCTION get_task_sources(p_task_id uuid) RETURNS TABLE(source_id uuid, source_type text, category text, config jsonb, credentials text)
    LANGUAGE plpgsql
    AS $$
BEGIN
  RETURN QUERY
  SELECT s.id as source_id, s.source_type, st.category, s.config, s.credentials_encrypted
  FROM tasks t
  CROSS JOIN LATERAL unnest(t.source_ids) AS task_source_id
  JOIN sources s ON s.id = task_source_id
  JOIN source_types st ON st.name = s.source_type
  WHERE t.id = p_task_id AND s.is_active = TRUE

  UNION

  SELECT s.id as source_id, s.source_type, st.category, s.config, s.credentials_encrypted
  FROM tasks t
  JOIN task_projects tp ON tp.task_id = t.id
  JOIN projects p ON p.id = tp.project_id
  JOIN sources s ON s.id = p.source_id
  JOIN source_types st ON st.name = s.source_type
  WHERE t.id = p_task_id AND s.is_active = TRUE AND (t.source_ids IS NULL OR t.source_ids = '{}');
END;
$$;

CREATE FUNCTION get_task_sources_by_category(p_task_id uuid, p_category text) RETURNS TABLE(source_id uuid, source_type text, config jsonb, credentials text)
    LANGUAGE plpgsql
    AS $$
BEGIN
  RETURN QUERY
  SELECT ts.source_id, ts.source_type, ts.config, ts.credentials
  FROM get_task_sources(p_task_id) ts WHERE ts.category = p_category;
END;
$$;

CREATE FUNCTION get_workspace_role(p_user_id uuid, p_workspace_id uuid) RETURNS text
    LANGUAGE plpgsql
    AS $$
DECLARE v_role TEXT;
BEGIN
  SELECT role INTO v_role FROM workspace_members WHERE user_id = p_user_id AND workspace_id = p_workspace_id AND is_active = TRUE;
  RETURN v_role;
END;
$$;

CREATE FUNCTION recover_orphaned_tasks() RETURNS integer
    LANGUAGE plpgsql
    AS $$
DECLARE
  v_count INTEGER;
BEGIN
  WITH orphaned AS (
    SELECT t.id FROM tasks t
    JOIN task_queue tq ON tq.task_id = t.id
    WHERE tq.worker_id IS NOT NULL AND t.status = 'in_progress' AND t.updated_at < NOW() - INTERVAL '10 minutes'
  )
  UPDATE task_queue tq
  SET worker_id = NULL, started_at = NULL, last_error = 'Worker timeout - task recovered'
  FROM orphaned o WHERE tq.task_id = o.id;

  GET DIAGNOSTICS v_count = ROW_COUNT;

  UPDATE tasks SET status = 'queued', worker_id = NULL, updated_at = NOW()
  WHERE id IN (SELECT task_id FROM task_queue WHERE worker_id IS NULL) AND status = 'in_progress';

  RETURN v_count;
END;
$$;

CREATE FUNCTION release_task(p_task_id uuid, p_error text DEFAULT NULL::text) RETURNS void
    LANGUAGE plpgsql
    AS $$
BEGIN
  UPDATE task_queue SET worker_id = NULL, started_at = NULL, last_error = COALESCE(p_error, last_error)
  WHERE task_id = p_task_id;

  UPDATE tasks SET status = 'queued', worker_id = NULL, updated_at = NOW()
  WHERE id = p_task_id;
END;
$$;

CREATE FUNCTION search_chat_history(query_vector vector, p_chat_id uuid, p_limit integer DEFAULT 10, p_threshold double precision DEFAULT 0.7) RETURNS TABLE(message_id uuid, similarity double precision, role text, content text, created_at timestamp without time zone)
    LANGUAGE plpgsql
    AS $$
BEGIN
  RETURN QUERY
  WITH ann AS (
    SELECT me.message_id, me.vector
    FROM message_embeddings me
    WHERE me.chat_id = p_chat_id
    ORDER BY me.vector_bit <~> binary_quantize(query_vector)::bit(1024)
    LIMIT GREATEST(p_limit * 8, 32)
  )
  SELECT m.id as message_id, (1 - (ann.vector <=> query_vector))::FLOAT as similarity,
         m.role, m.content, m.created_at
  FROM ann
  JOIN messages m ON m.id = ann.message_id
  WHERE (1 - (ann.vector <=> query_vector)) >= p_threshold
  ORDER BY ann.vector <=> query_vector
  LIMIT p_limit;
END;
$$;

CREATE FUNCTION search_content_embeddings(query_vector vector, p_limit integer DEFAULT 10, p_threshold double precision DEFAULT 0.7, p_source_ids uuid[] DEFAULT NULL::uuid[], p_workspace_id uuid DEFAULT NULL::uuid, p_categories text[] DEFAULT NULL::text[]) RETURNS TABLE(chunk_id uuid, content_item_id uuid, source_id uuid, similarity double precision, chunk_text text, item_uri text, item_title text, item_metadata jsonb)
    LANGUAGE plpgsql
    AS $$
BEGIN
  RETURN QUERY
  WITH ann AS (
    SELECT e.chunk_id, e.content_item_id, e.source_id, e.vector
    FROM embeddings e
    WHERE (p_workspace_id IS NULL OR e.workspace_id = p_workspace_id)
      AND (p_source_ids IS NULL OR e.source_id = ANY(p_source_ids))
    ORDER BY e.vector_bit <~> binary_quantize(query_vector)::bit(1024)
    LIMIT GREATEST(p_limit * 8, 32)
  )
  SELECT ann.chunk_id, ann.content_item_id, ann.source_id,
         (1 - (ann.vector <=> query_vector))::FLOAT as similarity,
         cc.text as chunk_text, ci.uri as item_uri, ci.title as item_title, ci.metadata as item_metadata
  FROM ann
  JOIN content_chunks cc ON cc.id = ann.chunk_id
  JOIN content_items ci ON ci.id = ann.content_item_id
  WHERE (1 - (ann.vector <=> query_vector)) >= p_threshold
    AND (p_categories IS NULL OR ci.category = ANY(p_categories))
  ORDER BY ann.vector <=> query_vector
  LIMIT p_limit;
END;
$$;

CREATE FUNCTION search_knowledge(query_vector vector, p_workspace_id uuid, p_limit integer DEFAULT 10, p_threshold double precision DEFAULT 0.7) RETURNS TABLE(entry_id uuid, similarity double precision, title text, content text, category text, tags text[])
    LANGUAGE plpgsql
    AS $$
BEGIN
  RETURN QUERY
  WITH ann AS (
    SELECT ke_embed.knowledge_entry_id, ke_embed.vector
    FROM knowledge_embeddings ke_embed
    WHERE ke_embed.workspace_id = p_workspace_id
    ORDER BY ke_embed.vector_bit <~> binary_quantize(query_vector)::bit(1024)
    LIMIT GREATEST(p_limit * 8, 32)
  )
  SELECT ke.id as entry_id, (1 - (ann.vector <=> query_vector))::FLOAT as similarity,
         ke.title, ke.content, ke.category, ke.tags
  FROM ann
  JOIN knowledge_entries ke ON ke.id = ann.knowledge_entry_id
  WHERE ke.is_active = TRUE
    AND (1 - (ann.vector <=> query_vector)) >= p_threshold
  ORDER BY ann.vector <=> query_vector
  LIMIT p_limit;
END;
$$;

CREATE FUNCTION update_chunk_search_vector() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
  NEW.search_vector := to_tsvector('english', COALESCE(NEW.text, ''));
  RETURN NEW;
END;
$$;

CREATE FUNCTION update_item_search_vector() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
  NEW.search_vector := setweight(to_tsvector('english', COALESCE(NEW.title, '')), 'A') ||
                       setweight(to_tsvector('english', COALESCE(NEW.content, '')), 'B');
  RETURN NEW;
END;
$$;

CREATE FUNCTION update_knowledge_search_vector() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
  NEW.search_vector := to_tsvector('english', COALESCE(NEW.title, '') || ' ' || COALESCE(NEW.content, ''));
  RETURN NEW;
END;
$$;

CREATE FUNCTION update_message_search_vector() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
  NEW.search_vector := to_tsvector('english', COALESCE(NEW.content, ''));
  RETURN NEW;
END;
$$;

CREATE FUNCTION update_sync_configs_updated_at() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
  NEW.updated_at = NOW();
  RETURN NEW;
END;
$$;

CREATE TABLE device_policy (
    id boolean DEFAULT true NOT NULL PRIMARY KEY,
    mode text DEFAULT 'open'::text NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT device_policy_mode_check CHECK ((mode = ANY (ARRAY['open'::text, 'allowed'::text]))),
    CONSTRAINT device_policy_singleton CHECK (id)
);

CREATE TABLE organizations (
    id uuid DEFAULT gen_random_uuid() NOT NULL PRIMARY KEY,
    name text NOT NULL,
    slug text NOT NULL UNIQUE,
    description text,
    is_active boolean DEFAULT true,
    created_at timestamp without time zone DEFAULT now(),
    updated_at timestamp without time zone DEFAULT now()
);
CREATE INDEX idx_organizations_active ON organizations (is_active) WHERE (is_active = true);

CREATE TABLE permissions (
    id uuid DEFAULT gen_random_uuid() NOT NULL PRIMARY KEY,
    name text NOT NULL UNIQUE,
    description text,
    resource text NOT NULL,
    action text NOT NULL,
    created_at timestamp without time zone DEFAULT now(),
    CONSTRAINT permissions_action_check CHECK ((action = ANY (ARRAY['create'::text, 'read'::text, 'update'::text, 'delete'::text])))
);
CREATE INDEX idx_permissions_resource ON permissions (resource);

CREATE TABLE plans (
    id uuid DEFAULT gen_random_uuid() NOT NULL PRIMARY KEY,
    name character varying(100) NOT NULL,
    slug character varying(50) NOT NULL UNIQUE,
    description text,
    price_monthly_cents integer NOT NULL,
    price_yearly_cents integer NOT NULL,
    is_active boolean DEFAULT true NOT NULL,
    is_public boolean DEFAULT true NOT NULL,
    features jsonb DEFAULT '{}'::jsonb NOT NULL,
    limits jsonb DEFAULT '{}'::jsonb NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL
);

CREATE TABLE roles (
    id uuid DEFAULT gen_random_uuid() NOT NULL PRIMARY KEY,
    name text NOT NULL UNIQUE,
    description text,
    is_system boolean DEFAULT false,
    created_at timestamp without time zone DEFAULT now(),
    updated_at timestamp without time zone DEFAULT now()
);

CREATE TABLE source_types (
    name text NOT NULL PRIMARY KEY,
    category text DEFAULT 'file'::text NOT NULL,
    description text NOT NULL,
    config_schema jsonb DEFAULT '{}'::jsonb NOT NULL,
    created_at timestamp without time zone DEFAULT now()
);
CREATE INDEX idx_source_types_category ON source_types (category);

CREATE TABLE users (
    id uuid DEFAULT gen_random_uuid() NOT NULL PRIMARY KEY,
    email text NOT NULL UNIQUE,
    password_hash text NOT NULL,
    display_name text,
    is_active boolean DEFAULT true,
    is_admin boolean DEFAULT false,
    email_verified boolean DEFAULT false NOT NULL,
    created_at timestamp without time zone DEFAULT now(),
    updated_at timestamp without time zone DEFAULT now(),
    last_login_at timestamp without time zone,
    time_zone text
);
CREATE INDEX idx_users_is_active ON users (is_active);

CREATE TABLE wiki_entries (
    id uuid DEFAULT gen_random_uuid() NOT NULL PRIMARY KEY,
    title text NOT NULL,
    content text NOT NULL,
    source_type text NOT NULL,
    source_id uuid,
    source_url text,
    created_at timestamp without time zone DEFAULT now(),
    updated_at timestamp without time zone DEFAULT now(),
    metadata jsonb DEFAULT '{}'::jsonb,
    CONSTRAINT wiki_entries_source_type_check CHECK ((source_type = ANY (ARRAY['chat'::text, 'manual'::text, 'url'::text, 'task'::text, 'github'::text])))
);
CREATE INDEX idx_wiki_entries_source_type ON wiki_entries (source_type);

CREATE TABLE agent_logins (
    id uuid DEFAULT gen_random_uuid() NOT NULL PRIMARY KEY,
    organization_id uuid NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    agent text NOT NULL,
    credential text,
    label text,
    expires_at timestamp with time zone,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    account text,
    windows jsonb,
    headroom double precision,
    usage_fetched_at timestamp with time zone,
    exhausted_until timestamp with time zone,
    last_used_at timestamp with time zone,
    CONSTRAINT agent_logins_agent_check CHECK ((agent = ANY (ARRAY['claude'::text, 'codex'::text])))
);
CREATE UNIQUE INDEX agent_logins_organization_agent_account_key ON agent_logins (organization_id, agent, account) WHERE (account IS NOT NULL);
CREATE INDEX idx_agent_logins_organization_agent ON agent_logins (organization_id, agent);

CREATE TABLE devices (
    id uuid DEFAULT gen_random_uuid() NOT NULL PRIMARY KEY,
    user_id uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    public_id uuid NOT NULL,
    name text,
    platform text NOT NULL,
    user_agent text,
    last_ip inet,
    last_seen_at timestamp with time zone DEFAULT now() NOT NULL,
    status text DEFAULT 'allowed'::text NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT devices_name_check CHECK (((name IS NULL) OR (length(TRIM(BOTH FROM name)) > 0))),
    CONSTRAINT devices_platform_check CHECK ((platform = ANY (ARRAY['android'::text, 'ios'::text, 'desktop'::text, 'browser'::text, 'cli'::text]))),
    CONSTRAINT devices_status_check CHECK ((status = ANY (ARRAY['allowed'::text, 'pending'::text, 'blocked'::text]))),
    CONSTRAINT devices_user_public_id_key UNIQUE (user_id, public_id)
);
CREATE INDEX devices_last_seen_at_idx ON devices (last_seen_at DESC);
CREATE INDEX devices_status_idx ON devices (status);
CREATE INDEX devices_user_id_idx ON devices (user_id);

CREATE TABLE email_verification_tokens (
    id uuid DEFAULT gen_random_uuid() NOT NULL PRIMARY KEY,
    user_id uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    token_hash character varying(255) NOT NULL UNIQUE,
    expires_at timestamp with time zone NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    used_at timestamp with time zone
);
CREATE INDEX idx_email_verification_tokens_user ON email_verification_tokens (user_id);

CREATE TABLE invitations (
    id uuid DEFAULT gen_random_uuid() NOT NULL PRIMARY KEY,
    email character varying(255) NOT NULL,
    organization_id uuid NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    workspace_ids uuid[] DEFAULT '{}'::uuid[],
    org_role character varying(50) DEFAULT 'member'::character varying NOT NULL,
    workspace_role character varying(50) DEFAULT 'member'::character varying NOT NULL,
    token_hash character varying(255) NOT NULL UNIQUE,
    invited_by uuid NOT NULL REFERENCES users(id),
    expires_at timestamp with time zone NOT NULL,
    accepted_at timestamp with time zone,
    created_at timestamp with time zone DEFAULT now() NOT NULL
);
CREATE INDEX idx_invitations_org ON invitations (organization_id);
CREATE INDEX idx_invitations_pending ON invitations (email, organization_id, expires_at) WHERE (accepted_at IS NULL);
CREATE INDEX idx_invitations_token ON invitations (token_hash);
CREATE UNIQUE INDEX invitations_pending_unique ON invitations (email, organization_id) WHERE (accepted_at IS NULL);

CREATE TABLE organization_ai_settings (
    id uuid DEFAULT gen_random_uuid() NOT NULL PRIMARY KEY,
    organization_id uuid NOT NULL UNIQUE REFERENCES organizations(id) ON DELETE CASCADE,
    provider text DEFAULT 'self_hosted'::text NOT NULL,
    litellm_host text,
    litellm_key text,
    openai_api_key text,
    openai_base_url text,
    anthropic_api_key text,
    anthropic_base_url text,
    bedrock_region text,
    bedrock_access_key text,
    bedrock_secret_key text,
    bedrock_use_iam_role boolean DEFAULT false,
    model_fast text,
    model_reasoning text,
    model_embedding text,
    created_at timestamp without time zone DEFAULT now(),
    updated_at timestamp without time zone DEFAULT now(),
    model_image text,
    model_video text,
    model_audio text,
    completions_routed boolean DEFAULT false NOT NULL,
    runpod_api_key text,
    CONSTRAINT organization_ai_settings_provider_check CHECK ((provider = ANY (ARRAY['self_hosted'::text, 'openai'::text, 'anthropic'::text, 'bedrock'::text, 'claude_code'::text, 'codex'::text])))
);
CREATE INDEX idx_org_ai_settings_org_id ON organization_ai_settings (organization_id);

CREATE TABLE organization_members (
    id uuid DEFAULT gen_random_uuid() NOT NULL PRIMARY KEY,
    organization_id uuid NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    user_id uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    role text DEFAULT 'member'::text NOT NULL,
    is_active boolean DEFAULT true NOT NULL,
    invited_by uuid REFERENCES users(id) ON DELETE SET NULL,
    invited_at timestamp without time zone,
    accepted_at timestamp without time zone,
    created_at timestamp without time zone DEFAULT now(),
    updated_at timestamp without time zone DEFAULT now(),
    CONSTRAINT organization_members_role_check CHECK ((role = ANY (ARRAY['owner'::text, 'admin'::text, 'member'::text]))),
    UNIQUE (organization_id, user_id)
);
CREATE INDEX idx_org_members_active ON organization_members (organization_id, is_active) WHERE (is_active = true);
CREATE INDEX idx_org_members_org ON organization_members (organization_id);
CREATE INDEX idx_org_members_user_active ON organization_members (user_id) WHERE (is_active = true);

CREATE TABLE password_reset_tokens (
    id uuid DEFAULT gen_random_uuid() NOT NULL PRIMARY KEY,
    user_id uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    token_hash character varying(255) NOT NULL UNIQUE,
    expires_at timestamp with time zone NOT NULL,
    used_at timestamp with time zone,
    created_at timestamp with time zone DEFAULT now() NOT NULL
);
CREATE INDEX idx_password_reset_tokens_user ON password_reset_tokens (user_id);

CREATE TABLE refresh_tokens (
    id uuid DEFAULT gen_random_uuid() NOT NULL PRIMARY KEY,
    user_id uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    token_hash text NOT NULL UNIQUE,
    expires_at timestamp without time zone NOT NULL,
    created_at timestamp without time zone DEFAULT now(),
    revoked_at timestamp without time zone,
    user_agent text,
    ip_address text
);
CREATE INDEX idx_refresh_tokens_expires_at ON refresh_tokens (expires_at);
CREATE INDEX idx_refresh_tokens_user_id ON refresh_tokens (user_id);

CREATE TABLE role_permissions (
    role_id uuid NOT NULL REFERENCES roles(id) ON DELETE CASCADE,
    permission_id uuid NOT NULL REFERENCES permissions(id) ON DELETE CASCADE,
    PRIMARY KEY (role_id, permission_id)
);
CREATE INDEX idx_role_permissions_role_id ON role_permissions (role_id);

CREATE TABLE subscriptions (
    id uuid DEFAULT gen_random_uuid() NOT NULL PRIMARY KEY,
    organization_id uuid NOT NULL UNIQUE REFERENCES organizations(id) ON DELETE CASCADE,
    plan_id uuid NOT NULL REFERENCES plans(id),
    status character varying(50) NOT NULL,
    current_period_start timestamp with time zone NOT NULL,
    current_period_end timestamp with time zone NOT NULL,
    cancel_at_period_end boolean DEFAULT false NOT NULL,
    canceled_at timestamp with time zone,
    trial_start timestamp with time zone,
    trial_end timestamp with time zone,
    stripe_subscription_id character varying(255),
    stripe_customer_id character varying(255),
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL
);
CREATE INDEX idx_subscriptions_status ON subscriptions (status);
CREATE INDEX idx_subscriptions_stripe ON subscriptions (stripe_subscription_id);

CREATE TABLE user_roles (
    user_id uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    role_id uuid NOT NULL REFERENCES roles(id) ON DELETE CASCADE,
    assigned_at timestamp without time zone DEFAULT now(),
    assigned_by uuid REFERENCES users(id),
    PRIMARY KEY (user_id, role_id)
);
CREATE INDEX idx_user_roles_role_id ON user_roles (role_id);
CREATE INDEX idx_user_roles_user_id ON user_roles (user_id);

CREATE TABLE wiki_chunks (
    id uuid DEFAULT gen_random_uuid() NOT NULL PRIMARY KEY,
    wiki_entry_id uuid NOT NULL REFERENCES wiki_entries(id) ON DELETE CASCADE,
    chunk_index integer NOT NULL,
    content text NOT NULL,
    embedding public.vector(1024),
    token_count integer,
    created_at timestamp without time zone DEFAULT now()
);
CREATE INDEX idx_wiki_chunks_embedding ON wiki_chunks USING ivfflat (embedding vector_cosine_ops) WITH (lists='100');
CREATE INDEX idx_wiki_chunks_entry_id ON wiki_chunks (wiki_entry_id);

CREATE TABLE workspaces (
    id uuid DEFAULT gen_random_uuid() NOT NULL PRIMARY KEY,
    organization_id uuid NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    name text NOT NULL,
    slug text NOT NULL,
    description text,
    is_active boolean DEFAULT true,
    created_at timestamp without time zone DEFAULT now(),
    updated_at timestamp without time zone DEFAULT now(),
    UNIQUE (organization_id, slug)
);
CREATE INDEX idx_workspaces_active ON workspaces (is_active) WHERE (is_active = true);
CREATE INDEX idx_workspaces_org_id ON workspaces (organization_id);
CREATE INDEX idx_workspaces_slug ON workspaces (slug);

CREATE TABLE audit_logs (
    id uuid DEFAULT gen_random_uuid() NOT NULL PRIMARY KEY,
    organization_id uuid REFERENCES organizations(id) ON DELETE SET NULL,
    workspace_id uuid REFERENCES workspaces(id) ON DELETE SET NULL,
    actor_id uuid REFERENCES users(id) ON DELETE SET NULL,
    actor_email character varying(255),
    action character varying(100) NOT NULL,
    resource_type character varying(50) NOT NULL,
    resource_id uuid,
    old_values jsonb,
    new_values jsonb,
    ip_address inet,
    user_agent text,
    created_at timestamp with time zone DEFAULT now() NOT NULL
);
CREATE INDEX idx_audit_logs_action ON audit_logs (action);
CREATE INDEX idx_audit_logs_actor ON audit_logs (actor_id);
CREATE INDEX idx_audit_logs_created ON audit_logs (created_at DESC);
CREATE INDEX idx_audit_logs_org ON audit_logs (organization_id, created_at DESC);
CREATE INDEX idx_audit_logs_resource ON audit_logs (resource_type, resource_id);

CREATE TABLE knowledge_entries (
    id uuid DEFAULT gen_random_uuid() NOT NULL PRIMARY KEY,
    workspace_id uuid NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    title text NOT NULL,
    content text NOT NULL,
    category text,
    tags text[] DEFAULT '{}'::text[],
    token_count integer DEFAULT 0 NOT NULL,
    is_active boolean DEFAULT true,
    source_url text,
    last_fetched_at timestamp without time zone,
    content_hash text,
    refresh_interval_minutes integer,
    last_fetch_error text,
    created_by uuid REFERENCES users(id) ON DELETE SET NULL,
    created_at timestamp without time zone DEFAULT now(),
    updated_at timestamp without time zone DEFAULT now(),
    search_vector tsvector,
    version bigint DEFAULT 0 NOT NULL,
    description text
);
CREATE INDEX idx_knowledge_documents_search ON knowledge_entries USING gin (to_tsvector('english'::regconfig, ((title || ' '::text) || COALESCE(content, ''::text)))) WHERE (is_active = true);
CREATE INDEX idx_knowledge_entries_search_vector ON knowledge_entries USING gin (search_vector) WHERE (is_active = true);
CREATE INDEX idx_knowledge_entries_tags ON knowledge_entries USING gin (tags);
CREATE UNIQUE INDEX idx_knowledge_memory_entry ON knowledge_entries (workspace_id, created_by, category, title) WHERE ((is_active = true) AND (category ~~ 'memory-%'::text));
CREATE INDEX idx_knowledge_refresh_due ON knowledge_entries (workspace_id, source_url) WHERE ((source_url IS NOT NULL) AND (is_active = true));
CREATE INDEX idx_knowledge_source_url ON knowledge_entries (workspace_id, source_url) WHERE (source_url IS NOT NULL);
CREATE INDEX idx_knowledge_workspace_category ON knowledge_entries (workspace_id, category, created_at DESC) WHERE (is_active = true);
CREATE TRIGGER knowledge_search_vector_trigger BEFORE INSERT OR UPDATE OF title, content ON knowledge_entries FOR EACH ROW EXECUTE FUNCTION update_knowledge_search_vector();

CREATE TABLE sessions (
    id uuid DEFAULT gen_random_uuid() NOT NULL PRIMARY KEY,
    user_id uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    refresh_token_hash character varying(255) NOT NULL UNIQUE,
    ip_address inet,
    user_agent text,
    device_info jsonb,
    last_active_at timestamp with time zone DEFAULT now() NOT NULL,
    expires_at timestamp with time zone NOT NULL,
    revoked_at timestamp with time zone,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    device_id uuid REFERENCES devices(id)
);
CREATE INDEX idx_sessions_expires ON sessions (expires_at);
CREATE INDEX idx_sessions_user_active ON sessions (user_id, last_active_at DESC) WHERE (revoked_at IS NULL);
CREATE INDEX sessions_device_id_idx ON sessions (device_id);

CREATE TABLE sources (
    id uuid DEFAULT gen_random_uuid() NOT NULL PRIMARY KEY,
    workspace_id uuid REFERENCES workspaces(id) ON DELETE CASCADE,
    name text NOT NULL,
    source_type text NOT NULL REFERENCES source_types(name),
    config jsonb DEFAULT '{}'::jsonb NOT NULL,
    credentials_encrypted text,
    description text,
    url text,
    is_active boolean DEFAULT true,
    last_verified_at timestamp without time zone,
    last_error text,
    created_at timestamp without time zone DEFAULT now(),
    updated_at timestamp without time zone DEFAULT now(),
    UNIQUE (name, source_type)
);
CREATE INDEX idx_sources_active ON sources (is_active) WHERE (is_active = true);
CREATE INDEX idx_sources_workspace_filters ON sources (workspace_id, source_type, is_active, created_at DESC);

CREATE TABLE usage_events (
    id uuid DEFAULT gen_random_uuid() NOT NULL PRIMARY KEY,
    organization_id uuid NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    workspace_id uuid REFERENCES workspaces(id) ON DELETE SET NULL,
    user_id uuid REFERENCES users(id) ON DELETE SET NULL,
    event_type character varying(50) NOT NULL,
    quantity bigint DEFAULT 1 NOT NULL,
    metadata jsonb DEFAULT '{}'::jsonb,
    recorded_at timestamp with time zone DEFAULT now() NOT NULL
);
CREATE INDEX idx_usage_events_org_time ON usage_events (organization_id, recorded_at);
CREATE INDEX idx_usage_events_org_type_time ON usage_events (organization_id, event_type, recorded_at);
CREATE INDEX idx_usage_events_type ON usage_events (event_type);

CREATE TABLE workspace_ai_settings (
    id uuid DEFAULT gen_random_uuid() NOT NULL PRIMARY KEY,
    workspace_id uuid NOT NULL UNIQUE REFERENCES workspaces(id) ON DELETE CASCADE,
    provider text,
    litellm_host text,
    litellm_key text,
    openai_api_key text,
    openai_base_url text,
    anthropic_api_key text,
    anthropic_base_url text,
    bedrock_region text,
    bedrock_access_key text,
    bedrock_secret_key text,
    bedrock_use_iam_role boolean,
    model_fast text,
    model_reasoning text,
    model_embedding text,
    created_at timestamp without time zone DEFAULT now(),
    updated_at timestamp without time zone DEFAULT now(),
    model_image text,
    model_video text,
    model_audio text,
    completions_routed boolean DEFAULT false NOT NULL,
    runpod_api_key text,
    CONSTRAINT workspace_ai_settings_provider_check CHECK (((provider IS NULL) OR (provider = ANY (ARRAY['self_hosted'::text, 'openai'::text, 'anthropic'::text, 'bedrock'::text, 'claude_code'::text, 'codex'::text]))))
);
CREATE INDEX idx_ws_ai_settings_ws_id ON workspace_ai_settings (workspace_id);

CREATE TABLE workspace_host_directories (
    workspace_id uuid NOT NULL PRIMARY KEY REFERENCES workspaces(id) ON DELETE CASCADE,
    directories text[] DEFAULT '{}'::text[] NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL
);

CREATE TABLE workspace_members (
    id uuid DEFAULT gen_random_uuid() NOT NULL PRIMARY KEY,
    workspace_id uuid NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    user_id uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    role text DEFAULT 'member'::text NOT NULL,
    is_active boolean DEFAULT true NOT NULL,
    invited_by uuid REFERENCES users(id) ON DELETE SET NULL,
    invited_at timestamp without time zone,
    accepted_at timestamp without time zone,
    created_at timestamp without time zone DEFAULT now(),
    updated_at timestamp without time zone DEFAULT now(),
    CONSTRAINT workspace_members_role_check CHECK ((role = ANY (ARRAY['owner'::text, 'admin'::text, 'member'::text, 'viewer'::text]))),
    UNIQUE (workspace_id, user_id)
);
CREATE INDEX idx_workspace_members_active ON workspace_members (workspace_id, is_active) WHERE (is_active = true);
CREATE INDEX idx_workspace_members_user_active ON workspace_members (user_id) WHERE (is_active = true);
CREATE INDEX idx_workspace_members_workspace ON workspace_members (workspace_id);

CREATE TABLE workspace_themes (
    id uuid DEFAULT gen_random_uuid() NOT NULL PRIMARY KEY,
    workspace_id uuid NOT NULL UNIQUE REFERENCES workspaces(id) ON DELETE CASCADE,
    primary_color_light text DEFAULT '#0011d9'::text,
    secondary_color_light text DEFAULT '#ecf9ff'::text,
    primary_color_dark text DEFAULT '#00f3ff'::text,
    secondary_color_dark text DEFAULT '#ecf9ff'::text,
    font_family text DEFAULT 'nunito'::text,
    font_size_base text DEFAULT '16px'::text,
    border_radius text DEFAULT 'large'::text,
    created_at timestamp without time zone DEFAULT now(),
    updated_at timestamp without time zone DEFAULT now()
);
CREATE INDEX idx_workspace_themes_workspace_id ON workspace_themes (workspace_id);

CREATE TABLE content_items (
    id uuid DEFAULT gen_random_uuid() NOT NULL PRIMARY KEY,
    source_id uuid NOT NULL REFERENCES sources(id) ON DELETE CASCADE,
    workspace_id uuid REFERENCES workspaces(id) ON DELETE CASCADE,
    category text NOT NULL,
    uri text NOT NULL,
    title text NOT NULL,
    content text,
    content_type text DEFAULT 'text/plain'::text NOT NULL,
    token_count integer DEFAULT 0 NOT NULL,
    metadata_only boolean DEFAULT false NOT NULL,
    content_hash text NOT NULL,
    metadata jsonb DEFAULT '{}'::jsonb NOT NULL,
    search_vector tsvector,
    modified_at timestamp without time zone,
    fetched_at timestamp without time zone DEFAULT now() NOT NULL,
    created_at timestamp without time zone DEFAULT now(),
    updated_at timestamp without time zone DEFAULT now(),
    UNIQUE (source_id, uri)
);
CREATE INDEX idx_content_documents_search ON content_items USING gin (to_tsvector('english'::regconfig, ((title || ' '::text) || COALESCE(
CASE
    WHEN metadata_only THEN NULL::text
    ELSE content
END, ''::text))));
CREATE INDEX idx_content_items_category ON content_items (category);
CREATE INDEX idx_content_items_fetched ON content_items (fetched_at);
CREATE INDEX idx_content_items_hash ON content_items (content_hash);
CREATE INDEX idx_content_items_metadata ON content_items USING gin (metadata);
CREATE INDEX idx_content_items_search ON content_items USING gin (search_vector);
CREATE INDEX idx_content_items_source ON content_items (source_id);
CREATE INDEX idx_content_items_workspace ON content_items (workspace_id);
CREATE TRIGGER item_search_vector_trigger BEFORE INSERT OR UPDATE OF title, content ON content_items FOR EACH ROW EXECUTE FUNCTION update_item_search_vector();

CREATE TABLE knowledge_embeddings (
    id uuid DEFAULT gen_random_uuid() NOT NULL PRIMARY KEY,
    knowledge_entry_id uuid NOT NULL UNIQUE REFERENCES knowledge_entries(id) ON DELETE CASCADE,
    workspace_id uuid NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    vector public.vector(1024) NOT NULL,
    model text NOT NULL,
    created_at timestamp without time zone DEFAULT now(),
    vector_bit bit(1024) GENERATED ALWAYS AS ((public.binary_quantize(vector))::bit(1024)) STORED
);
CREATE INDEX idx_knowledge_embeddings_vector ON knowledge_embeddings USING hnsw (vector vector_cosine_ops) WITH (m='16', ef_construction='64');
CREATE INDEX idx_knowledge_embeddings_vector_bit ON knowledge_embeddings USING hnsw (vector_bit bit_hamming_ops) WITH (m='16', ef_construction='64');
CREATE INDEX idx_knowledge_embeddings_workspace ON knowledge_embeddings (workspace_id);

CREATE TABLE projects (
    id uuid DEFAULT gen_random_uuid() NOT NULL PRIMARY KEY,
    workspace_id uuid REFERENCES workspaces(id) ON DELETE CASCADE,
    source_id uuid REFERENCES sources(id) ON DELETE SET NULL,
    name text NOT NULL,
    description text,
    status text DEFAULT 'active'::text NOT NULL,
    github_repo_url text,
    github_access_token text,
    created_at timestamp without time zone DEFAULT now(),
    updated_at timestamp without time zone DEFAULT now(),
    auto boolean DEFAULT false NOT NULL,
    auto_actor_id uuid REFERENCES users(id) ON DELETE SET NULL,
    brief jsonb,
    auto_paused_reason text,
    auto_claimed_at timestamp with time zone,
    auto_completed_at timestamp with time zone,
    "position" integer DEFAULT 0 NOT NULL,
    CONSTRAINT projects_status_check CHECK ((status = ANY (ARRAY['active'::text, 'on_hold'::text, 'cancelled'::text])))
);
CREATE INDEX idx_projects_auto ON projects (id) WHERE auto;
CREATE INDEX idx_projects_status ON projects (status);
CREATE INDEX idx_projects_workspace_position ON projects (workspace_id, "position");
CREATE INDEX idx_projects_workspace_status ON projects (workspace_id, status, created_at DESC);

CREATE TABLE source_sync_state (
    source_id uuid NOT NULL PRIMARY KEY REFERENCES sources(id) ON DELETE CASCADE,
    last_sync_at timestamp without time zone,
    cursor text,
    etag text,
    version text,
    extra jsonb DEFAULT '{}'::jsonb NOT NULL,
    created_at timestamp without time zone DEFAULT now(),
    updated_at timestamp without time zone DEFAULT now()
);
CREATE INDEX idx_sync_state_last_sync ON source_sync_state (last_sync_at);

CREATE TABLE chats (
    id uuid DEFAULT gen_random_uuid() NOT NULL PRIMARY KEY,
    workspace_id uuid REFERENCES workspaces(id) ON DELETE CASCADE,
    title text NOT NULL,
    model_name text NOT NULL,
    created_at timestamp without time zone DEFAULT now(),
    updated_at timestamp without time zone DEFAULT now(),
    archived boolean DEFAULT false,
    agent_enabled boolean DEFAULT false NOT NULL,
    agent_sandboxed boolean DEFAULT true NOT NULL,
    automatic_title boolean DEFAULT false NOT NULL,
    title_message_id uuid,
    auto_approve boolean DEFAULT false NOT NULL,
    "character" jsonb,
    reasoning_effort text DEFAULT 'auto'::text NOT NULL,
    purpose text DEFAULT 'assistant'::text NOT NULL,
    project_id uuid REFERENCES projects(id) ON DELETE SET NULL,
    offline boolean DEFAULT false NOT NULL,
    context_tokens bigint,
    agent_login_id uuid REFERENCES agent_logins(id) ON DELETE SET NULL,
    agent_session_id text,
    agent_session_agent text,
    agent_session_entry bigint,
    agent_session_prompt text,
    provider text,
    provider_model text,
    used_at timestamp without time zone DEFAULT now() NOT NULL,
    CONSTRAINT chats_context_tokens_check CHECK (((context_tokens IS NULL) OR (context_tokens > 0))),
    CONSTRAINT chats_provider_check CHECK (((provider IS NULL) OR (provider = ANY (ARRAY['self_hosted'::text, 'openai'::text, 'anthropic'::text, 'bedrock'::text, 'claude_code'::text, 'codex'::text])))),
    CONSTRAINT chats_provider_model_check CHECK (((provider_model IS NULL) OR ((provider IS NOT NULL) AND (btrim(provider_model) <> ''::text)))),
    CONSTRAINT chats_purpose_check CHECK ((purpose = ANY (ARRAY['assistant'::text, 'project_planner'::text, 'project_updates'::text]))),
    CONSTRAINT chats_reasoning_effort_check CHECK ((reasoning_effort = ANY (ARRAY['auto'::text, 'off'::text, 'low'::text, 'medium'::text, 'high'::text, 'xhigh'::text, 'max'::text, 'ultra'::text])))
);
CREATE INDEX idx_chats_project ON chats (project_id) WHERE (project_id IS NOT NULL);
CREATE INDEX idx_chats_workspace_archived ON chats (workspace_id, archived, updated_at DESC);
CREATE INDEX idx_chats_workspace_used ON chats (workspace_id, used_at DESC) WHERE ((purpose = 'assistant'::text) AND (archived IS NOT TRUE));

CREATE TABLE content_chunks (
    id uuid DEFAULT gen_random_uuid() NOT NULL PRIMARY KEY,
    content_item_id uuid NOT NULL REFERENCES content_items(id) ON DELETE CASCADE,
    chunk_index integer NOT NULL,
    text text NOT NULL,
    token_count integer NOT NULL,
    start_offset integer NOT NULL,
    end_offset integer NOT NULL,
    search_vector tsvector,
    created_at timestamp without time zone DEFAULT now(),
    UNIQUE (content_item_id, chunk_index)
);
CREATE INDEX idx_content_chunks_item ON content_chunks (content_item_id);
CREATE INDEX idx_content_chunks_search ON content_chunks USING gin (search_vector);
CREATE TRIGGER chunk_search_vector_trigger BEFORE INSERT OR UPDATE OF text ON content_chunks FOR EACH ROW EXECUTE FUNCTION update_chunk_search_vector();

CREATE TABLE heuristic_analysis (
    id uuid DEFAULT gen_random_uuid() NOT NULL PRIMARY KEY,
    content_item_id uuid NOT NULL UNIQUE REFERENCES content_items(id) ON DELETE CASCADE,
    entities jsonb DEFAULT '{}'::jsonb NOT NULL,
    categorization jsonb DEFAULT '{}'::jsonb NOT NULL,
    quality jsonb DEFAULT '{}'::jsonb NOT NULL,
    analyzed_at timestamp without time zone DEFAULT now() NOT NULL,
    created_at timestamp without time zone DEFAULT now(),
    updated_at timestamp without time zone DEFAULT now()
);
CREATE INDEX idx_heuristic_analysis_categorization ON heuristic_analysis USING gin (categorization);
CREATE INDEX idx_heuristic_analysis_entities ON heuristic_analysis USING gin (entities);
CREATE INDEX idx_heuristic_analysis_item ON heuristic_analysis (content_item_id);

CREATE TABLE sync_configs (
    id uuid DEFAULT gen_random_uuid() NOT NULL PRIMARY KEY,
    project_id uuid NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    provider text NOT NULL,
    enabled boolean DEFAULT true NOT NULL,
    config jsonb DEFAULT '{}'::jsonb NOT NULL,
    webhook_secret_encrypted text,
    created_at timestamp without time zone DEFAULT now(),
    updated_at timestamp without time zone DEFAULT now(),
    CONSTRAINT sync_configs_provider_check CHECK ((provider = ANY (ARRAY['github'::text, 'linear'::text]))),
    UNIQUE (project_id, provider)
);
CREATE INDEX idx_sync_configs_enabled ON sync_configs (enabled) WHERE (enabled = true);
CREATE INDEX idx_sync_configs_project_id ON sync_configs (project_id);
CREATE INDEX idx_sync_configs_provider ON sync_configs (provider);
CREATE TRIGGER sync_configs_updated_at BEFORE UPDATE ON sync_configs FOR EACH ROW EXECUTE FUNCTION update_sync_configs_updated_at();

CREATE TABLE chat_agent_contexts (
    chat_id uuid NOT NULL PRIMARY KEY REFERENCES chats(id) ON DELETE CASCADE,
    agent text NOT NULL,
    model text NOT NULL,
    window_tokens bigint NOT NULL,
    used_tokens bigint,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT chat_agent_contexts_agent_check CHECK ((agent = ANY (ARRAY['claude'::text, 'codex'::text]))),
    CONSTRAINT chat_agent_contexts_used_tokens_check CHECK (((used_tokens IS NULL) OR (used_tokens > 0))),
    CONSTRAINT chat_agent_contexts_window_tokens_check CHECK ((window_tokens > 0))
);

CREATE TABLE chat_attached_sources (
    chat_id uuid NOT NULL REFERENCES chats(id) ON DELETE CASCADE,
    source_id uuid NOT NULL REFERENCES sources(id) ON DELETE CASCADE,
    attached_at timestamp with time zone DEFAULT now() NOT NULL,
    PRIMARY KEY (chat_id, source_id)
);
CREATE INDEX idx_chat_attached_sources_source ON chat_attached_sources (source_id);

CREATE TABLE chat_checkpoints (
    chat_id uuid NOT NULL PRIMARY KEY REFERENCES chats(id) ON DELETE CASCADE,
    revision bigint NOT NULL,
    content text NOT NULL,
    entries jsonb NOT NULL,
    fingerprint text NOT NULL,
    updated_at timestamp with time zone DEFAULT clock_timestamp() NOT NULL,
    CONSTRAINT chat_checkpoints_content_check CHECK ((length(TRIM(BOTH FROM content)) > 0)),
    CONSTRAINT chat_checkpoints_entries_check CHECK ((jsonb_typeof(entries) = 'array'::text)),
    CONSTRAINT chat_checkpoints_revision_check CHECK ((revision > 0))
);

CREATE TABLE chat_leases (
    chat_id uuid NOT NULL PRIMARY KEY REFERENCES chats(id) ON DELETE CASCADE,
    owner uuid NOT NULL,
    fence bigint NOT NULL,
    expires_at timestamp with time zone NOT NULL,
    CONSTRAINT chat_leases_fence_check CHECK ((fence > 0))
);

CREATE TABLE chat_sources (
    chat_id uuid NOT NULL REFERENCES chats(id) ON DELETE CASCADE,
    identifier text NOT NULL,
    kind text NOT NULL,
    key text NOT NULL,
    uri text NOT NULL,
    title text NOT NULL,
    first_observed_at timestamp with time zone DEFAULT clock_timestamp() NOT NULL,
    last_observed_at timestamp with time zone DEFAULT clock_timestamp() NOT NULL,
    CONSTRAINT chat_sources_kind_check CHECK ((kind = ANY (ARRAY['web'::text, 'doc'::text, 'kb'::text, 'chat'::text]))),
    CONSTRAINT chat_sources_identity UNIQUE (chat_id, kind, key),
    PRIMARY KEY (chat_id, identifier)
);

CREATE TABLE chat_subagents (
    id uuid DEFAULT gen_random_uuid() NOT NULL PRIMARY KEY,
    chat_id uuid NOT NULL REFERENCES chats(id) ON DELETE CASCADE,
    message_id uuid,
    external_id text NOT NULL,
    description text DEFAULT ''::text NOT NULL,
    kind text,
    model text,
    background boolean DEFAULT false NOT NULL,
    status text DEFAULT 'running'::text NOT NULL,
    tool_uses bigint,
    duration_ms bigint,
    tokens bigint,
    last_tool text,
    summary text,
    error text,
    revision bigint DEFAULT 0 NOT NULL,
    started_at timestamp with time zone DEFAULT now() NOT NULL,
    finished_at timestamp with time zone,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT chat_subagents_status_check CHECK ((status = ANY (ARRAY['running'::text, 'completed'::text, 'failed'::text, 'stopped'::text])))
);
CREATE UNIQUE INDEX chat_subagents_chat_external_key ON chat_subagents (chat_id, external_id);
CREATE INDEX chat_subagents_chat_started_idx ON chat_subagents (chat_id, started_at);
CREATE INDEX chat_subagents_running_idx ON chat_subagents (chat_id, message_id) WHERE (status = 'running'::text);

CREATE TABLE embeddings (
    id uuid DEFAULT gen_random_uuid() NOT NULL PRIMARY KEY,
    chunk_id uuid NOT NULL UNIQUE REFERENCES content_chunks(id) ON DELETE CASCADE,
    content_item_id uuid NOT NULL REFERENCES content_items(id) ON DELETE CASCADE,
    source_id uuid NOT NULL REFERENCES sources(id) ON DELETE CASCADE,
    workspace_id uuid REFERENCES workspaces(id) ON DELETE CASCADE,
    vector public.vector(1024) NOT NULL,
    model text NOT NULL,
    created_at timestamp without time zone DEFAULT now(),
    vector_bit bit(1024) GENERATED ALWAYS AS ((public.binary_quantize(vector))::bit(1024)) STORED
);
CREATE INDEX idx_embeddings_content_item ON embeddings (content_item_id);
CREATE INDEX idx_embeddings_source ON embeddings (source_id);
CREATE INDEX idx_embeddings_vector ON embeddings USING hnsw (vector vector_cosine_ops) WITH (m='16', ef_construction='64');
CREATE INDEX idx_embeddings_vector_bit ON embeddings USING hnsw (vector_bit bit_hamming_ops) WITH (m='16', ef_construction='64');
CREATE INDEX idx_embeddings_workspace ON embeddings (workspace_id);
CREATE INDEX idx_embeddings_workspace_source ON embeddings (workspace_id, source_id);

CREATE TABLE messages (
    id uuid DEFAULT gen_random_uuid() NOT NULL PRIMARY KEY,
    chat_id uuid NOT NULL REFERENCES chats(id) ON DELETE CASCADE,
    role text NOT NULL,
    content text NOT NULL,
    created_at timestamp without time zone DEFAULT now(),
    metadata jsonb DEFAULT '{}'::jsonb,
    search_vector tsvector,
    CONSTRAINT messages_role_check CHECK ((role = ANY (ARRAY['user'::text, 'assistant'::text, 'system'::text])))
);
CREATE INDEX idx_messages_chat_created ON messages (chat_id, created_at);
CREATE INDEX idx_messages_search_vector ON messages USING gin (search_vector);
CREATE INDEX idx_messages_typed ON messages (chat_id, created_at DESC) WHERE ((role = 'user'::text) AND (NOT COALESCE(((jsonb_typeof(metadata) = 'object'::text) AND (metadata ? 'source'::text)), false)));
CREATE TRIGGER message_search_vector_trigger BEFORE INSERT OR UPDATE OF content ON messages FOR EACH ROW EXECUTE FUNCTION update_message_search_vector();

CREATE TABLE sync_deliveries (
    sync_config_id uuid NOT NULL REFERENCES sync_configs(id) ON DELETE CASCADE,
    delivery_id text NOT NULL,
    received_at timestamp with time zone DEFAULT now() NOT NULL,
    PRIMARY KEY (sync_config_id, delivery_id)
);

CREATE TABLE sync_unlinked_items (
    sync_config_id uuid NOT NULL REFERENCES sync_configs(id) ON DELETE CASCADE,
    external_id text NOT NULL,
    unlinked_at timestamp with time zone DEFAULT now() NOT NULL,
    PRIMARY KEY (sync_config_id, external_id)
);

CREATE TABLE chat_subagent_steps (
    subagent_id uuid NOT NULL REFERENCES chat_subagents(id) ON DELETE CASCADE,
    step_id text NOT NULL,
    "position" bigint NOT NULL,
    name text NOT NULL,
    subject text,
    status text DEFAULT 'running'::text NOT NULL,
    CONSTRAINT chat_subagent_steps_status_check CHECK ((status = ANY (ARRAY['running'::text, 'succeeded'::text, 'failed'::text]))),
    PRIMARY KEY (subagent_id, step_id)
);
CREATE INDEX chat_subagent_steps_order_idx ON chat_subagent_steps (subagent_id, "position");

CREATE TABLE chat_turns (
    id uuid NOT NULL PRIMARY KEY,
    chat_id uuid NOT NULL REFERENCES chats(id) ON DELETE CASCADE,
    user_message_id uuid NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
    fence bigint NOT NULL,
    version smallint DEFAULT 1 NOT NULL,
    status text DEFAULT 'running'::text NOT NULL,
    created_at timestamp with time zone DEFAULT clock_timestamp() NOT NULL,
    completed_at timestamp with time zone,
    CONSTRAINT chat_turns_status_check CHECK ((status = ANY (ARRAY['running'::text, 'completed'::text, 'interrupted'::text]))),
    CONSTRAINT chat_turns_version_check CHECK ((version = 1)),
    UNIQUE (chat_id, id),
    UNIQUE (chat_id, user_message_id)
);
CREATE INDEX chat_turns_running ON chat_turns (chat_id) WHERE (status = 'running'::text);

CREATE TABLE message_embeddings (
    id uuid DEFAULT gen_random_uuid() NOT NULL PRIMARY KEY,
    message_id uuid NOT NULL UNIQUE REFERENCES messages(id) ON DELETE CASCADE,
    chat_id uuid NOT NULL REFERENCES chats(id) ON DELETE CASCADE,
    vector public.vector(1024) NOT NULL,
    model text NOT NULL,
    created_at timestamp without time zone DEFAULT now(),
    vector_bit bit(1024) GENERATED ALWAYS AS ((public.binary_quantize(vector))::bit(1024)) STORED,
    workspace_id uuid REFERENCES workspaces(id) ON DELETE CASCADE
);
CREATE INDEX idx_message_embeddings_chat ON message_embeddings (chat_id);
CREATE INDEX idx_message_embeddings_vector ON message_embeddings USING hnsw (vector vector_cosine_ops) WITH (m='16', ef_construction='64');
CREATE INDEX idx_message_embeddings_vector_bit ON message_embeddings USING hnsw (vector_bit bit_hamming_ops) WITH (m='16', ef_construction='64');
CREATE INDEX idx_message_embeddings_workspace_chat ON message_embeddings (workspace_id, chat_id);

CREATE TABLE reminders (
    id uuid DEFAULT gen_random_uuid() NOT NULL PRIMARY KEY,
    workspace_id uuid NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    chat_id uuid NOT NULL REFERENCES chats(id) ON DELETE CASCADE,
    created_by uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    content text NOT NULL,
    due_at timestamp with time zone NOT NULL,
    status text DEFAULT 'pending'::text NOT NULL,
    message_id uuid UNIQUE REFERENCES messages(id) ON DELETE SET NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    completed_at timestamp with time zone,
    rrule text,
    prompt text,
    timing_mode text DEFAULT 'exact_schedule'::text NOT NULL,
    anchor_at timestamp with time zone,
    expires_at timestamp with time zone,
    fired_count integer DEFAULT 0 NOT NULL,
    last_fired_at timestamp with time zone,
    last_observation text,
    time_zone text DEFAULT 'UTC'::text NOT NULL,
    CONSTRAINT reminders_content_check CHECK ((length(TRIM(BOTH FROM content)) > 0)),
    CONSTRAINT reminders_last_observation_check CHECK (((last_observation IS NULL) OR (length(last_observation) <= 4000))),
    CONSTRAINT reminders_status_check CHECK ((status = ANY (ARRAY['pending'::text, 'delivered'::text, 'cancelled'::text, 'expired'::text]))),
    CONSTRAINT reminders_timing_mode_check CHECK ((timing_mode = ANY (ARRAY['exact_schedule'::text, 'condition_watch'::text]))),
    CONSTRAINT reminders_watch_compares_check CHECK (((timing_mode <> 'condition_watch'::text) OR ((COALESCE(rrule, ''::text) ~ '[^[:space:]]'::text) AND (COALESCE(prompt, ''::text) ~ '[^[:space:]]'::text))))
);
CREATE INDEX reminders_due ON reminders (due_at) WHERE (status = 'pending'::text);

CREATE TABLE chat_entries (
    chat_id uuid NOT NULL REFERENCES chats(id) ON DELETE CASCADE,
    id text NOT NULL,
    "position" bigint GENERATED ALWAYS AS IDENTITY,
    turn_id uuid,
    message jsonb NOT NULL,
    consumed boolean DEFAULT false NOT NULL,
    legacy boolean DEFAULT false NOT NULL,
    created_at timestamp with time zone DEFAULT clock_timestamp() NOT NULL,
    CONSTRAINT chat_entries_message_check CHECK (((message ->> 'version'::text) = '1'::text)),
    CONSTRAINT chat_entries_message_check1 CHECK (((message ->> 'role'::text) = ANY (ARRAY['system'::text, 'user'::text, 'assistant'::text, 'tool'::text]))),
    UNIQUE (chat_id, "position"),
    PRIMARY KEY (chat_id, id),
    FOREIGN KEY (chat_id, turn_id) REFERENCES chat_turns(chat_id, id) ON DELETE CASCADE
);

CREATE TABLE reminder_turns (
    id uuid DEFAULT gen_random_uuid() NOT NULL PRIMARY KEY,
    reminder_id uuid NOT NULL REFERENCES reminders(id) ON DELETE CASCADE,
    workspace_id uuid NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    chat_id uuid NOT NULL REFERENCES chats(id) ON DELETE CASCADE,
    created_by uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    prompt text NOT NULL,
    attempts integer DEFAULT 0 NOT NULL,
    claimed_at timestamp with time zone,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT reminder_turns_prompt_check CHECK ((length(TRIM(BOTH FROM prompt)) > 0))
);
CREATE INDEX reminder_turns_queue ON reminder_turns (created_at, id);
CREATE INDEX reminder_turns_reminder ON reminder_turns (reminder_id);

CREATE TABLE chat_calls (
    chat_id uuid NOT NULL,
    id text NOT NULL,
    turn_id uuid NOT NULL,
    envelope_id text NOT NULL,
    result_id text,
    mutating boolean NOT NULL,
    UNIQUE (chat_id, result_id),
    PRIMARY KEY (chat_id, id),
    FOREIGN KEY (chat_id, envelope_id) REFERENCES chat_entries(chat_id, id) ON DELETE CASCADE,
    FOREIGN KEY (chat_id, result_id) REFERENCES chat_entries(chat_id, id) ON DELETE CASCADE,
    FOREIGN KEY (chat_id, turn_id) REFERENCES chat_turns(chat_id, id) ON DELETE CASCADE
);
CREATE INDEX chat_calls_pending ON chat_calls (chat_id, turn_id) WHERE (result_id IS NULL);

CREATE TABLE task_runs (
    id uuid DEFAULT gen_random_uuid() NOT NULL PRIMARY KEY,
    task_id uuid NOT NULL,
    status text NOT NULL,
    current_phase text,
    progress_percent integer DEFAULT 0,
    started_at timestamp without time zone DEFAULT now(),
    completed_at timestamp without time zone,
    error_message text,
    artifacts jsonb DEFAULT '{}'::jsonb,
    result_summary text,
    modified_files jsonb,
    heartbeat_at timestamp with time zone DEFAULT now() NOT NULL,
    owner uuid,
    triggered_by uuid REFERENCES users(id) ON DELETE SET NULL,
    pending_question jsonb,
    pending_wait jsonb,
    plan text,
    model text,
    unattended boolean DEFAULT false NOT NULL,
    CONSTRAINT task_runs_progress_percent_check CHECK (((progress_percent >= 0) AND (progress_percent <= 100))),
    CONSTRAINT task_runs_status_check CHECK ((status = ANY (ARRAY['running'::text, 'waiting'::text, 'completed'::text, 'failed'::text, 'cancelled'::text])))
);
CREATE INDEX idx_task_runs_status ON task_runs (status);
CREATE INDEX idx_task_runs_task_started ON task_runs (task_id, started_at DESC);
CREATE UNIQUE INDEX task_runs_active_waiting ON task_runs (task_id) WHERE (status = ANY (ARRAY['running'::text, 'waiting'::text]));
CREATE INDEX task_runs_heartbeat_waiting ON task_runs (heartbeat_at) WHERE (status = ANY (ARRAY['running'::text, 'waiting'::text]));

CREATE TABLE chat_pull_requests (
    id uuid DEFAULT gen_random_uuid() NOT NULL PRIMARY KEY,
    chat_id uuid NOT NULL REFERENCES chats(id) ON DELETE CASCADE,
    provider text DEFAULT 'github'::text NOT NULL,
    owner text NOT NULL,
    repo text NOT NULL,
    number bigint NOT NULL,
    url text NOT NULL,
    title text,
    source_id uuid REFERENCES sources(id) ON DELETE SET NULL,
    dismissed_at timestamp with time zone,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    auto_fix boolean DEFAULT false NOT NULL,
    auto_fix_run_id uuid REFERENCES task_runs(id) ON DELETE SET NULL,
    auto_fix_by uuid REFERENCES users(id) ON DELETE SET NULL,
    auto_fix_head text,
    CONSTRAINT chat_pull_requests_number_check CHECK ((number > 0)),
    CONSTRAINT chat_pull_requests_provider_check CHECK ((provider = ANY (ARRAY['github'::text])))
);
CREATE UNIQUE INDEX chat_pull_requests_chat_pull_request_key ON chat_pull_requests (chat_id, provider, lower(owner), lower(repo), number);
CREATE INDEX chat_pull_requests_source_id_idx ON chat_pull_requests (source_id);

CREATE TABLE task_file_changes (
    id uuid DEFAULT gen_random_uuid() NOT NULL PRIMARY KEY,
    task_run_id uuid NOT NULL REFERENCES task_runs(id) ON DELETE CASCADE,
    file_path text NOT NULL,
    change_type text NOT NULL,
    original_content text,
    new_content text,
    diff text,
    applied boolean DEFAULT false,
    applied_at timestamp without time zone,
    reverted boolean DEFAULT false,
    reverted_at timestamp without time zone,
    created_at timestamp without time zone DEFAULT now(),
    CONSTRAINT task_file_changes_change_type_check CHECK ((change_type = ANY (ARRAY['create'::text, 'modify'::text, 'delete'::text])))
);
CREATE INDEX idx_task_file_changes_applied ON task_file_changes (applied);
CREATE INDEX idx_task_file_changes_run_id ON task_file_changes (task_run_id);

CREATE TABLE task_run_checkouts (
    run_id uuid NOT NULL PRIMARY KEY REFERENCES task_runs(id) ON DELETE CASCADE,
    head text NOT NULL,
    published text,
    updated_at timestamp with time zone DEFAULT now() NOT NULL
);

CREATE TABLE task_run_logs (
    id uuid DEFAULT gen_random_uuid() NOT NULL PRIMARY KEY,
    task_run_id uuid NOT NULL REFERENCES task_runs(id) ON DELETE CASCADE,
    phase text NOT NULL,
    agent_type text NOT NULL,
    log_level text NOT NULL,
    message text NOT NULL,
    metadata jsonb DEFAULT '{}'::jsonb,
    created_at timestamp without time zone DEFAULT now(),
    CONSTRAINT task_run_logs_log_level_check CHECK ((log_level = ANY (ARRAY['debug'::text, 'info'::text, 'warning'::text, 'error'::text])))
);
CREATE INDEX idx_task_run_logs_run_created ON task_run_logs (task_run_id, created_at);

CREATE TABLE task_tool_calls (
    id uuid DEFAULT gen_random_uuid() NOT NULL PRIMARY KEY,
    task_run_id uuid NOT NULL REFERENCES task_runs(id) ON DELETE CASCADE,
    tool_name text NOT NULL,
    tool_input jsonb NOT NULL,
    tool_output jsonb,
    status text DEFAULT 'pending'::text NOT NULL,
    error_message text,
    started_at timestamp without time zone DEFAULT now(),
    completed_at timestamp without time zone,
    CONSTRAINT task_tool_calls_status_check CHECK ((status = ANY (ARRAY['pending'::text, 'running'::text, 'completed'::text, 'failed'::text])))
);
CREATE INDEX idx_task_tool_calls_run_id ON task_tool_calls (task_run_id);
CREATE INDEX idx_task_tool_calls_status ON task_tool_calls (status);

CREATE TABLE tasks (
    id uuid DEFAULT gen_random_uuid() NOT NULL PRIMARY KEY,
    workspace_id uuid NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    title text NOT NULL,
    description text NOT NULL,
    acceptance_criteria text,
    status text DEFAULT 'created'::text NOT NULL,
    priority integer,
    model_name text,
    dependencies jsonb DEFAULT '[]'::jsonb,
    is_agentic boolean DEFAULT false NOT NULL,
    github_repo_url text,
    source_id uuid REFERENCES sources(id) ON DELETE SET NULL,
    source_ids uuid[] DEFAULT '{}'::uuid[],
    worker_id text,
    queued_at timestamp without time zone,
    started_at timestamp without time zone,
    completed_at timestamp without time zone,
    created_at timestamp without time zone DEFAULT now(),
    updated_at timestamp without time zone DEFAULT now(),
    pr_url text,
    branch_name text,
    pr_status text,
    pr_created_at timestamp without time zone,
    assignee_id uuid REFERENCES users(id) ON DELETE SET NULL,
    created_by uuid REFERENCES users(id) ON DELETE SET NULL,
    active_run_id uuid REFERENCES task_runs(id) ON DELETE SET NULL,
    require_plan_approval boolean DEFAULT false NOT NULL,
    title_pending boolean DEFAULT false NOT NULL,
    chat_id uuid REFERENCES chats(id) ON DELETE SET NULL,
    origin text DEFAULT 'member'::text NOT NULL,
    CONSTRAINT tasks_origin_check CHECK ((origin = ANY (ARRAY['member'::text, 'reminder'::text, 'automation'::text]))),
    CONSTRAINT tasks_priority_check CHECK (((priority >= 1) AND (priority <= 5))),
    CONSTRAINT tasks_status_check CHECK ((status = ANY (ARRAY['created'::text, 'queued'::text, 'in_progress'::text, 'review'::text, 'complete'::text, 'blocked'::text])))
);
CREATE INDEX idx_tasks_branch_name ON tasks (branch_name) WHERE (branch_name IS NOT NULL);
CREATE INDEX idx_tasks_pr_status ON tasks (pr_status) WHERE (pr_status IS NOT NULL);
CREATE INDEX idx_tasks_queued_at ON tasks (queued_at) WHERE (queued_at IS NOT NULL);
CREATE INDEX idx_tasks_source_ids ON tasks USING gin (source_ids);
CREATE INDEX idx_tasks_status ON tasks (status);
CREATE INDEX idx_tasks_worker_id ON tasks (worker_id) WHERE (worker_id IS NOT NULL);
CREATE INDEX idx_tasks_workspace_created ON tasks (workspace_id, created_at DESC);
CREATE INDEX idx_tasks_workspace_status ON tasks (workspace_id, status, created_at DESC);
CREATE INDEX tasks_chat_id_idx ON tasks (chat_id, created_at DESC) WHERE (chat_id IS NOT NULL);

CREATE TABLE context_gatherings (
    id uuid DEFAULT gen_random_uuid() NOT NULL PRIMARY KEY,
    workspace_id uuid REFERENCES workspaces(id) ON DELETE SET NULL,
    task_id uuid REFERENCES tasks(id) ON DELETE SET NULL,
    user_id uuid REFERENCES users(id) ON DELETE SET NULL,
    status text DEFAULT 'pending'::text NOT NULL,
    source_ids uuid[] DEFAULT '{}'::uuid[] NOT NULL,
    config jsonb DEFAULT '{}'::jsonb NOT NULL,
    stats jsonb,
    error_message text,
    started_at timestamp without time zone,
    completed_at timestamp without time zone,
    created_at timestamp without time zone DEFAULT now(),
    CONSTRAINT context_gatherings_status_check CHECK ((status = ANY (ARRAY['pending'::text, 'running'::text, 'completed'::text, 'failed'::text])))
);
CREATE INDEX idx_context_gatherings_source_ids ON context_gatherings USING gin (source_ids);
CREATE INDEX idx_context_gatherings_status ON context_gatherings (status);
CREATE INDEX idx_context_gatherings_task ON context_gatherings (task_id);
CREATE INDEX idx_context_gatherings_user ON context_gatherings (user_id);
CREATE INDEX idx_context_gatherings_workspace ON context_gatherings (workspace_id);

CREATE TABLE synced_items (
    id uuid DEFAULT gen_random_uuid() NOT NULL PRIMARY KEY,
    sync_config_id uuid NOT NULL REFERENCES sync_configs(id) ON DELETE CASCADE,
    task_id uuid NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    external_id text NOT NULL,
    external_url text,
    last_synced_at timestamp without time zone DEFAULT now(),
    sync_direction text DEFAULT 'bidirectional'::text NOT NULL,
    last_external_state jsonb,
    created_at timestamp without time zone DEFAULT now(),
    CONSTRAINT synced_items_sync_direction_check CHECK ((sync_direction = ANY (ARRAY['outbound'::text, 'inbound'::text, 'bidirectional'::text]))),
    UNIQUE (sync_config_id, external_id),
    UNIQUE (sync_config_id, task_id)
);
CREATE INDEX idx_synced_items_external_id ON synced_items (sync_config_id, external_id);
CREATE INDEX idx_synced_items_last_synced ON synced_items (last_synced_at);
CREATE INDEX idx_synced_items_sync_config_id ON synced_items (sync_config_id);
CREATE INDEX idx_synced_items_task_id ON synced_items (task_id);

CREATE TABLE task_automation (
    task_id uuid NOT NULL PRIMARY KEY REFERENCES tasks(id) ON DELETE CASCADE,
    project_id uuid NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    kind text,
    stage text DEFAULT 'idle'::text NOT NULL,
    reason text,
    runs integer DEFAULT 0 NOT NULL,
    review_rounds integer DEFAULT 0 NOT NULL,
    head text,
    checks text,
    checks_since timestamp with time zone,
    bot_trigger_head text,
    merge_sha text,
    auto_created boolean DEFAULT false NOT NULL,
    last_run_id uuid REFERENCES task_runs(id) ON DELETE SET NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT task_automation_kind_check CHECK ((kind = ANY (ARRAY['scaffold'::text, 'ci'::text, 'tests'::text, 'feature'::text, 'deployment'::text, 'docs'::text, 'fix'::text]))),
    CONSTRAINT task_automation_stage_check CHECK ((stage = ANY (ARRAY['idle'::text, 'running'::text, 'no_changes'::text, 'awaiting_checks'::text, 'awaiting_reviews'::text, 'fixing'::text, 'merging'::text, 'post_merge'::text, 'merged'::text, 'paused'::text])))
);
CREATE INDEX idx_task_automation_project_stage ON task_automation (project_id, stage);

CREATE TABLE task_projects (
    task_id uuid NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    project_id uuid NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    created_at timestamp without time zone DEFAULT now(),
    PRIMARY KEY (task_id, project_id)
);
CREATE INDEX idx_task_projects_project_id ON task_projects (project_id);
CREATE INDEX idx_task_projects_task_id ON task_projects (task_id);

CREATE TABLE task_queue (
    id uuid DEFAULT gen_random_uuid() NOT NULL PRIMARY KEY,
    task_id uuid NOT NULL UNIQUE REFERENCES tasks(id) ON DELETE CASCADE,
    priority integer DEFAULT 3 NOT NULL,
    queued_at timestamp without time zone DEFAULT now(),
    started_at timestamp without time zone,
    worker_id text,
    attempts integer DEFAULT 0,
    max_attempts integer DEFAULT 3,
    last_error text
);
CREATE INDEX idx_task_queue_priority ON task_queue (priority DESC, queued_at);
CREATE INDEX idx_task_queue_worker ON task_queue (worker_id) WHERE (worker_id IS NOT NULL);

CREATE TABLE task_reviews (
    id uuid DEFAULT gen_random_uuid() NOT NULL PRIMARY KEY,
    task_id uuid NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    run_id uuid REFERENCES task_runs(id) ON DELETE SET NULL,
    round integer NOT NULL,
    head text NOT NULL,
    reviewer_kind text DEFAULT 'model'::text NOT NULL,
    reviewer text NOT NULL,
    author_model text,
    same_model boolean DEFAULT false NOT NULL,
    verdict text NOT NULL,
    summary text DEFAULT ''::text NOT NULL,
    findings jsonb DEFAULT '[]'::jsonb NOT NULL,
    addressed jsonb DEFAULT '[]'::jsonb NOT NULL,
    external_id text,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT task_reviews_reviewer_kind_check CHECK ((reviewer_kind = ANY (ARRAY['model'::text, 'bot'::text]))),
    CONSTRAINT task_reviews_verdict_check CHECK ((verdict = ANY (ARRAY['approve'::text, 'request_changes'::text, 'unparseable'::text, 'failed'::text]))),
    UNIQUE (task_id, round)
);

CREATE TABLE gathering_events (
    id uuid DEFAULT gen_random_uuid() NOT NULL PRIMARY KEY,
    gathering_id uuid NOT NULL REFERENCES context_gatherings(id) ON DELETE CASCADE,
    event_type text NOT NULL,
    payload jsonb DEFAULT '{}'::jsonb NOT NULL,
    created_at timestamp without time zone DEFAULT now()
);
CREATE INDEX idx_gathering_events_created ON gathering_events (created_at);
CREATE INDEX idx_gathering_events_gathering ON gathering_events (gathering_id);
CREATE INDEX idx_gathering_events_gathering_created ON gathering_events (gathering_id, created_at);

CREATE TABLE sync_events (
    id uuid DEFAULT gen_random_uuid() NOT NULL PRIMARY KEY,
    sync_config_id uuid NOT NULL REFERENCES sync_configs(id) ON DELETE CASCADE,
    synced_item_id uuid REFERENCES synced_items(id) ON DELETE SET NULL,
    event_type text NOT NULL,
    direction text NOT NULL,
    payload jsonb,
    error_message text,
    created_at timestamp without time zone DEFAULT now(),
    CONSTRAINT sync_events_direction_check CHECK ((direction = ANY (ARRAY['outbound'::text, 'inbound'::text]))),
    CONSTRAINT sync_events_event_type_check CHECK ((event_type = ANY (ARRAY['create'::text, 'update'::text, 'close'::text, 'unlink'::text, 'webhook_received'::text, 'sync_error'::text])))
);
CREATE INDEX idx_sync_events_created_at ON sync_events (created_at DESC);
CREATE INDEX idx_sync_events_event_type ON sync_events (event_type);
CREATE INDEX idx_sync_events_sync_config_id ON sync_events (sync_config_id);
CREATE INDEX idx_sync_events_synced_item_id ON sync_events (synced_item_id);

ALTER TABLE task_runs
    ADD CONSTRAINT task_runs_task_id_fkey FOREIGN KEY (task_id) REFERENCES tasks(id) ON DELETE CASCADE;

INSERT INTO source_types (name, category, description, config_schema) VALUES
  ('github', 'file', 'GitHub repository', '{"type":"object","required":["owner","repo"],"properties":{"owner":{"type":"string"},"repo":{"type":"string"},"branch":{"type":"string","default":"main"},"base_path":{"type":"string","default":""}}}'),
  ('gitlab', 'file', 'GitLab repository', '{"type":"object","required":["project_id"],"properties":{"project_id":{"type":"string"},"host":{"type":"string","default":"https://gitlab.com"},"branch":{"type":"string","default":"main"},"base_path":{"type":"string","default":""}}}'),
  ('filesystem', 'file', 'Local filesystem (self-hosted only)', '{"type":"object","required":["base_path"],"properties":{"base_path":{"type":"string"},"allow_writes":{"type":"boolean","default":true}}}'),
  ('ical', 'calendar', 'iCalendar subscription URL', '{"type":"object","required":["url"],"properties":{"url":{"type":"string"},"refresh_interval":{"type":"integer","default":3600}}}'),
  ('imap', 'mail', 'IMAP mail server', '{"type":"object","required":["host","port","username"],"properties":{"host":{"type":"string"},"port":{"type":"integer","default":993},"username":{"type":"string"},"use_ssl":{"type":"boolean","default":true},"folder":{"type":"string","default":"INBOX"}}}'),
  ('discord', 'chat', 'Discord server', '{"type":"object","required":["server_id"],"properties":{"server_id":{"type":"string"},"channel_ids":{"type":"array","items":{"type":"string"}}}}'),
  ('slack', 'chat', 'Slack workspace', '{"type":"object","required":["workspace_id"],"properties":{"workspace_id":{"type":"string"},"channel_ids":{"type":"array","items":{"type":"string"}}}}'),
  ('web', 'web', 'Web URL content fetcher', '{"type":"object","required":["url"],"properties":{"url":{"type":"string"},"headers":{"type":"object"}}}'),
  ('text', 'text', 'Raw text/string content', '{"type":"object","required":["content"],"properties":{"content":{"type":"string"},"label":{"type":"string"}}}');

INSERT INTO permissions (name, description, resource, action) VALUES
  ('projects:create', 'Create new projects', 'projects', 'create'),
  ('projects:read', 'View projects', 'projects', 'read'),
  ('projects:update', 'Update existing projects', 'projects', 'update'),
  ('projects:delete', 'Delete projects', 'projects', 'delete'),
  ('tasks:create', 'Create new tasks', 'tasks', 'create'),
  ('tasks:read', 'View tasks', 'tasks', 'read'),
  ('tasks:update', 'Update existing tasks', 'tasks', 'update'),
  ('tasks:delete', 'Delete tasks', 'tasks', 'delete'),
  ('chats:create', 'Create new chats', 'chats', 'create'),
  ('chats:read', 'View chats', 'chats', 'read'),
  ('chats:update', 'Update existing chats', 'chats', 'update'),
  ('chats:delete', 'Delete chats', 'chats', 'delete'),
  ('sources:create', 'Create new sources', 'sources', 'create'),
  ('sources:read', 'View sources', 'sources', 'read'),
  ('sources:update', 'Update existing sources', 'sources', 'update'),
  ('sources:delete', 'Delete sources', 'sources', 'delete'),
  ('models:create', 'Pull/install new models', 'models', 'create'),
  ('models:read', 'View installed models', 'models', 'read'),
  ('models:update', 'Update model settings', 'models', 'update'),
  ('models:delete', 'Delete/remove models', 'models', 'delete'),
  ('wiki:create', 'Create wiki entries', 'wiki', 'create'),
  ('wiki:read', 'View wiki entries', 'wiki', 'read'),
  ('wiki:update', 'Update wiki entries', 'wiki', 'update'),
  ('wiki:delete', 'Delete wiki entries', 'wiki', 'delete'),
  ('users:create', 'Create new users', 'users', 'create'),
  ('users:read', 'View users', 'users', 'read'),
  ('users:update', 'Update users', 'users', 'update'),
  ('users:delete', 'Delete users', 'users', 'delete'),
  ('organizations:create', 'Create new organizations', 'organizations', 'create'),
  ('organizations:read', 'View organizations', 'organizations', 'read'),
  ('organizations:update', 'Update organization settings', 'organizations', 'update'),
  ('organizations:delete', 'Delete organizations', 'organizations', 'delete'),
  ('workspaces:create', 'Create new workspaces', 'workspaces', 'create'),
  ('workspaces:read', 'View workspaces', 'workspaces', 'read'),
  ('workspaces:update', 'Update workspace settings', 'workspaces', 'update'),
  ('workspaces:delete', 'Delete workspaces', 'workspaces', 'delete');

INSERT INTO roles (id, name, description, is_system) VALUES
  ('00000000-0000-0000-0000-000000000001', 'admin', 'Full system access', TRUE),
  ('00000000-0000-0000-0000-000000000002', 'user', 'Standard user access', TRUE),
  ('00000000-0000-0000-0000-000000000003', 'viewer', 'Read-only access', TRUE);

INSERT INTO role_permissions (role_id, permission_id)
SELECT '00000000-0000-0000-0000-000000000001', id FROM permissions;

INSERT INTO role_permissions (role_id, permission_id)
SELECT '00000000-0000-0000-0000-000000000002', id FROM permissions
WHERE name IN (
  'projects:create', 'projects:read', 'projects:update', 'projects:delete',
  'tasks:create', 'tasks:read', 'tasks:update', 'tasks:delete',
  'chats:create', 'chats:read', 'chats:update', 'chats:delete',
  'sources:read', 'models:read', 'wiki:read', 'wiki:create', 'wiki:update',
  'organizations:create', 'organizations:read', 'organizations:update',
  'workspaces:create', 'workspaces:read', 'workspaces:update'
);

INSERT INTO role_permissions (role_id, permission_id)
SELECT '00000000-0000-0000-0000-000000000003', id FROM permissions WHERE action = 'read';

INSERT INTO organizations (id, name, slug, description, is_active) VALUES
  ('00000000-0000-0000-0000-000000000001', 'Default Organization', 'default', 'Default organization for self-hosted installation', true);

INSERT INTO workspaces (id, organization_id, name, slug, description, is_active) VALUES
  ('00000000-0000-0000-0000-000000000001', '00000000-0000-0000-0000-000000000001', 'Default Workspace', 'default', 'Default workspace for self-hosted installation', true);

INSERT INTO plans (name, slug, description, price_monthly_cents, price_yearly_cents, features, limits) VALUES
  ('Free', 'free', 'For individuals and small teams', 0, 0, '{"api_access": true}'::jsonb, '{"max_workspaces": 1, "max_members": 3, "max_chats_per_month": 100}'::jsonb),
  ('Pro', 'pro', 'For growing teams', 2900, 29000, '{"api_access": true, "priority_support": true}'::jsonb, '{"max_workspaces": 10, "max_members": 25, "max_chats_per_month": 5000}'::jsonb),
  ('Enterprise', 'enterprise', 'For large organizations', 9900, 99000, '{"api_access": true, "priority_support": true, "sso": true, "audit_log": true}'::jsonb, '{"max_workspaces": -1, "max_members": -1, "max_chats_per_month": -1}'::jsonb);

INSERT INTO device_policy (id, mode) VALUES (true, 'open');
