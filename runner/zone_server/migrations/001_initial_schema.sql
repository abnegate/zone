-- Zone initial schema.
-- Fresh installs apply this single migration. sqlx owns the transaction.

CREATE EXTENSION IF NOT EXISTS vector WITH SCHEMA public;


--
-- Name: check_organization_membership(uuid, uuid); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.check_organization_membership(p_user_id uuid, p_organization_id uuid) RETURNS boolean
    LANGUAGE plpgsql
    AS $$
BEGIN
  RETURN EXISTS(SELECT 1 FROM organization_members WHERE user_id = p_user_id AND organization_id = p_organization_id AND is_active = TRUE);
END;
$$;


--
-- Name: check_workspace_membership(uuid, uuid); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.check_workspace_membership(p_user_id uuid, p_workspace_id uuid) RETURNS boolean
    LANGUAGE plpgsql
    AS $$
BEGIN
  RETURN EXISTS(SELECT 1 FROM workspace_members WHERE user_id = p_user_id AND workspace_id = p_workspace_id AND is_active = TRUE);
END;
$$;


--
-- Name: claim_next_task(text); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.claim_next_task(p_worker_id text) RETURNS TABLE(task_id uuid, queue_id uuid)
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


--
-- Name: complete_task_in_queue(uuid, boolean); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.complete_task_in_queue(p_task_id uuid, p_success boolean) RETURNS void
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


--
-- Name: get_organization_role(uuid, uuid); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.get_organization_role(p_user_id uuid, p_organization_id uuid) RETURNS text
    LANGUAGE plpgsql
    AS $$
DECLARE v_role TEXT;
BEGIN
  SELECT role INTO v_role FROM organization_members WHERE user_id = p_user_id AND organization_id = p_organization_id AND is_active = TRUE;
  RETURN v_role;
END;
$$;


--
-- Name: get_task_source(uuid); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.get_task_source(p_task_id uuid) RETURNS TABLE(source_id uuid, source_type text, config jsonb, credentials text)
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


--
-- Name: get_task_sources(uuid); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.get_task_sources(p_task_id uuid) RETURNS TABLE(source_id uuid, source_type text, category text, config jsonb, credentials text)
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


--
-- Name: get_task_sources_by_category(uuid, text); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.get_task_sources_by_category(p_task_id uuid, p_category text) RETURNS TABLE(source_id uuid, source_type text, config jsonb, credentials text)
    LANGUAGE plpgsql
    AS $$
BEGIN
  RETURN QUERY
  SELECT ts.source_id, ts.source_type, ts.config, ts.credentials
  FROM get_task_sources(p_task_id) ts WHERE ts.category = p_category;
END;
$$;


--
-- Name: get_workspace_role(uuid, uuid); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.get_workspace_role(p_user_id uuid, p_workspace_id uuid) RETURNS text
    LANGUAGE plpgsql
    AS $$
DECLARE v_role TEXT;
BEGIN
  SELECT role INTO v_role FROM workspace_members WHERE user_id = p_user_id AND workspace_id = p_workspace_id AND is_active = TRUE;
  RETURN v_role;
END;
$$;


--
-- Name: recover_orphaned_tasks(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.recover_orphaned_tasks() RETURNS integer
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


--
-- Name: release_task(uuid, text); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.release_task(p_task_id uuid, p_error text DEFAULT NULL::text) RETURNS void
    LANGUAGE plpgsql
    AS $$
BEGIN
  UPDATE task_queue SET worker_id = NULL, started_at = NULL, last_error = COALESCE(p_error, last_error)
  WHERE task_id = p_task_id;

  UPDATE tasks SET status = 'queued', worker_id = NULL, updated_at = NOW()
  WHERE id = p_task_id;
END;
$$;


--
-- Name: search_chat_history(public.vector, uuid, integer, double precision); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.search_chat_history(query_vector public.vector, p_chat_id uuid, p_limit integer DEFAULT 10, p_threshold double precision DEFAULT 0.7) RETURNS TABLE(message_id uuid, similarity double precision, role text, content text, created_at timestamp without time zone)
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


--
-- Name: search_content_embeddings(public.vector, integer, double precision, uuid[], uuid, text[]); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.search_content_embeddings(query_vector public.vector, p_limit integer DEFAULT 10, p_threshold double precision DEFAULT 0.7, p_source_ids uuid[] DEFAULT NULL::uuid[], p_workspace_id uuid DEFAULT NULL::uuid, p_categories text[] DEFAULT NULL::text[]) RETURNS TABLE(chunk_id uuid, content_item_id uuid, source_id uuid, similarity double precision, chunk_text text, item_uri text, item_title text, item_metadata jsonb)
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


--
-- Name: search_knowledge(public.vector, uuid, integer, double precision); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.search_knowledge(query_vector public.vector, p_workspace_id uuid, p_limit integer DEFAULT 10, p_threshold double precision DEFAULT 0.7) RETURNS TABLE(entry_id uuid, similarity double precision, title text, content text, category text, tags text[])
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


--
-- Name: update_chunk_search_vector(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.update_chunk_search_vector() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
  NEW.search_vector := to_tsvector('english', COALESCE(NEW.text, ''));
  RETURN NEW;
END;
$$;


--
-- Name: update_item_search_vector(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.update_item_search_vector() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
  NEW.search_vector := setweight(to_tsvector('english', COALESCE(NEW.title, '')), 'A') ||
                       setweight(to_tsvector('english', COALESCE(NEW.content, '')), 'B');
  RETURN NEW;
END;
$$;


--
-- Name: update_knowledge_search_vector(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.update_knowledge_search_vector() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
  NEW.search_vector := to_tsvector('english', COALESCE(NEW.title, '') || ' ' || COALESCE(NEW.content, ''));
  RETURN NEW;
END;
$$;


--
-- Name: update_message_search_vector(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.update_message_search_vector() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
  NEW.search_vector := to_tsvector('english', COALESCE(NEW.content, ''));
  RETURN NEW;
END;
$$;


--
-- Name: update_sync_configs_updated_at(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.update_sync_configs_updated_at() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
  NEW.updated_at = NOW();
  RETURN NEW;
END;
$$;


--
-- Name: agent_logins; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.agent_logins (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    organization_id uuid NOT NULL,
    agent text NOT NULL,
    credential text,
    label text,
    expires_at timestamp with time zone,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT agent_logins_agent_check CHECK ((agent = ANY (ARRAY['claude'::text, 'codex'::text])))
);


--
-- Name: audit_logs; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.audit_logs (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    organization_id uuid,
    workspace_id uuid,
    actor_id uuid,
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


--
-- Name: chat_attached_sources; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.chat_attached_sources (
    chat_id uuid NOT NULL,
    source_id uuid NOT NULL,
    attached_at timestamp with time zone DEFAULT now() NOT NULL
);


--
-- Name: chat_calls; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.chat_calls (
    chat_id uuid NOT NULL,
    id text NOT NULL,
    turn_id uuid NOT NULL,
    envelope_id text NOT NULL,
    result_id text,
    mutating boolean NOT NULL
);


--
-- Name: chat_checkpoints; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.chat_checkpoints (
    chat_id uuid NOT NULL,
    revision bigint NOT NULL,
    content text NOT NULL,
    entries jsonb NOT NULL,
    fingerprint text NOT NULL,
    updated_at timestamp with time zone DEFAULT clock_timestamp() NOT NULL,
    CONSTRAINT chat_checkpoints_content_check CHECK ((length(TRIM(BOTH FROM content)) > 0)),
    CONSTRAINT chat_checkpoints_entries_check CHECK ((jsonb_typeof(entries) = 'array'::text)),
    CONSTRAINT chat_checkpoints_revision_check CHECK ((revision > 0))
);


--
-- Name: chat_entries; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.chat_entries (
    chat_id uuid NOT NULL,
    id text NOT NULL,
    "position" bigint NOT NULL,
    turn_id uuid,
    message jsonb NOT NULL,
    consumed boolean DEFAULT false NOT NULL,
    legacy boolean DEFAULT false NOT NULL,
    created_at timestamp with time zone DEFAULT clock_timestamp() NOT NULL,
    CONSTRAINT chat_entries_message_check CHECK (((message ->> 'version'::text) = '1'::text)),
    CONSTRAINT chat_entries_message_check1 CHECK (((message ->> 'role'::text) = ANY (ARRAY['system'::text, 'user'::text, 'assistant'::text, 'tool'::text])))
);


--
-- Name: chat_entries_position_seq; Type: SEQUENCE; Schema: public; Owner: -
--

ALTER TABLE public.chat_entries ALTER COLUMN "position" ADD GENERATED ALWAYS AS IDENTITY (
    SEQUENCE NAME public.chat_entries_position_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1
);


--
-- Name: chat_leases; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.chat_leases (
    chat_id uuid NOT NULL,
    owner uuid NOT NULL,
    fence bigint NOT NULL,
    expires_at timestamp with time zone NOT NULL,
    CONSTRAINT chat_leases_fence_check CHECK ((fence > 0))
);


--
-- Name: chat_sources; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.chat_sources (
    chat_id uuid NOT NULL,
    identifier text NOT NULL,
    kind text NOT NULL,
    key text NOT NULL,
    uri text NOT NULL,
    title text NOT NULL,
    first_observed_at timestamp with time zone DEFAULT clock_timestamp() NOT NULL,
    last_observed_at timestamp with time zone DEFAULT clock_timestamp() NOT NULL,
    CONSTRAINT chat_sources_kind_check CHECK ((kind = ANY (ARRAY['web'::text, 'doc'::text, 'kb'::text, 'chat'::text])))
);


--
-- Name: chat_turns; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.chat_turns (
    id uuid NOT NULL,
    chat_id uuid NOT NULL,
    user_message_id uuid NOT NULL,
    fence bigint NOT NULL,
    version smallint DEFAULT 1 NOT NULL,
    status text DEFAULT 'running'::text NOT NULL,
    created_at timestamp with time zone DEFAULT clock_timestamp() NOT NULL,
    completed_at timestamp with time zone,
    CONSTRAINT chat_turns_status_check CHECK ((status = ANY (ARRAY['running'::text, 'completed'::text, 'interrupted'::text]))),
    CONSTRAINT chat_turns_version_check CHECK ((version = 1))
);


--
-- Name: chats; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.chats (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    workspace_id uuid,
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
    project_id uuid,
    CONSTRAINT chats_purpose_check CHECK ((purpose = ANY (ARRAY['assistant'::text, 'project_planner'::text, 'project_updates'::text]))),
    CONSTRAINT chats_reasoning_effort_check CHECK ((reasoning_effort = ANY (ARRAY['auto'::text, 'off'::text, 'low'::text, 'medium'::text, 'high'::text])))
);


--
-- Name: content_chunks; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.content_chunks (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    content_item_id uuid NOT NULL,
    chunk_index integer NOT NULL,
    text text NOT NULL,
    token_count integer NOT NULL,
    start_offset integer NOT NULL,
    end_offset integer NOT NULL,
    search_vector tsvector,
    created_at timestamp without time zone DEFAULT now()
);


--
-- Name: content_items; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.content_items (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    source_id uuid NOT NULL,
    workspace_id uuid,
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
    updated_at timestamp without time zone DEFAULT now()
);


--
-- Name: context_gatherings; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.context_gatherings (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    workspace_id uuid,
    task_id uuid,
    user_id uuid,
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


--
-- Name: email_verification_tokens; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.email_verification_tokens (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    user_id uuid NOT NULL,
    token_hash character varying(255) NOT NULL,
    expires_at timestamp with time zone NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    used_at timestamp with time zone
);


--
-- Name: embeddings; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.embeddings (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    chunk_id uuid NOT NULL,
    content_item_id uuid NOT NULL,
    source_id uuid NOT NULL,
    workspace_id uuid,
    vector public.vector(1024) NOT NULL,
    model text NOT NULL,
    created_at timestamp without time zone DEFAULT now(),
    vector_bit bit(1024) GENERATED ALWAYS AS ((public.binary_quantize(vector))::bit(1024)) STORED
);


--
-- Name: gathering_events; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.gathering_events (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    gathering_id uuid NOT NULL,
    event_type text NOT NULL,
    payload jsonb DEFAULT '{}'::jsonb NOT NULL,
    created_at timestamp without time zone DEFAULT now()
);


--
-- Name: heuristic_analysis; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.heuristic_analysis (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    content_item_id uuid NOT NULL,
    entities jsonb DEFAULT '{}'::jsonb NOT NULL,
    categorization jsonb DEFAULT '{}'::jsonb NOT NULL,
    quality jsonb DEFAULT '{}'::jsonb NOT NULL,
    analyzed_at timestamp without time zone DEFAULT now() NOT NULL,
    created_at timestamp without time zone DEFAULT now(),
    updated_at timestamp without time zone DEFAULT now()
);


--
-- Name: invitations; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.invitations (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    email character varying(255) NOT NULL,
    organization_id uuid NOT NULL,
    workspace_ids uuid[] DEFAULT '{}'::uuid[],
    org_role character varying(50) DEFAULT 'member'::character varying NOT NULL,
    workspace_role character varying(50) DEFAULT 'member'::character varying NOT NULL,
    token_hash character varying(255) NOT NULL,
    invited_by uuid NOT NULL,
    expires_at timestamp with time zone NOT NULL,
    accepted_at timestamp with time zone,
    created_at timestamp with time zone DEFAULT now() NOT NULL
);


--
-- Name: knowledge_embeddings; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.knowledge_embeddings (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    knowledge_entry_id uuid NOT NULL,
    workspace_id uuid NOT NULL,
    vector public.vector(1024) NOT NULL,
    model text NOT NULL,
    created_at timestamp without time zone DEFAULT now(),
    vector_bit bit(1024) GENERATED ALWAYS AS ((public.binary_quantize(vector))::bit(1024)) STORED
);


--
-- Name: knowledge_entries; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.knowledge_entries (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    workspace_id uuid NOT NULL,
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
    created_by uuid,
    created_at timestamp without time zone DEFAULT now(),
    updated_at timestamp without time zone DEFAULT now(),
    search_vector tsvector,
    version bigint DEFAULT 0 NOT NULL,
    description text
);


--
-- Name: message_embeddings; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.message_embeddings (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    message_id uuid NOT NULL,
    chat_id uuid NOT NULL,
    vector public.vector(1024) NOT NULL,
    model text NOT NULL,
    created_at timestamp without time zone DEFAULT now(),
    vector_bit bit(1024) GENERATED ALWAYS AS ((public.binary_quantize(vector))::bit(1024)) STORED,
    workspace_id uuid
);


--
-- Name: messages; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.messages (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    chat_id uuid NOT NULL,
    role text NOT NULL,
    content text NOT NULL,
    created_at timestamp without time zone DEFAULT now(),
    metadata jsonb DEFAULT '{}'::jsonb,
    search_vector tsvector,
    CONSTRAINT messages_role_check CHECK ((role = ANY (ARRAY['user'::text, 'assistant'::text, 'system'::text])))
);


--
-- Name: organization_ai_settings; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.organization_ai_settings (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    organization_id uuid NOT NULL,
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
    CONSTRAINT organization_ai_settings_provider_check CHECK ((provider = ANY (ARRAY['self_hosted'::text, 'openai'::text, 'anthropic'::text, 'bedrock'::text, 'claude_code'::text, 'codex'::text])))
);


--
-- Name: organization_members; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.organization_members (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    organization_id uuid NOT NULL,
    user_id uuid NOT NULL,
    role text DEFAULT 'member'::text NOT NULL,
    is_active boolean DEFAULT true NOT NULL,
    invited_by uuid,
    invited_at timestamp without time zone,
    accepted_at timestamp without time zone,
    created_at timestamp without time zone DEFAULT now(),
    updated_at timestamp without time zone DEFAULT now(),
    CONSTRAINT organization_members_role_check CHECK ((role = ANY (ARRAY['owner'::text, 'admin'::text, 'member'::text])))
);


--
-- Name: organizations; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.organizations (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    name text NOT NULL,
    slug text NOT NULL,
    description text,
    is_active boolean DEFAULT true,
    created_at timestamp without time zone DEFAULT now(),
    updated_at timestamp without time zone DEFAULT now()
);


--
-- Name: password_reset_tokens; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.password_reset_tokens (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    user_id uuid NOT NULL,
    token_hash character varying(255) NOT NULL,
    expires_at timestamp with time zone NOT NULL,
    used_at timestamp with time zone,
    created_at timestamp with time zone DEFAULT now() NOT NULL
);


--
-- Name: permissions; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.permissions (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    name text NOT NULL,
    description text,
    resource text NOT NULL,
    action text NOT NULL,
    created_at timestamp without time zone DEFAULT now(),
    CONSTRAINT permissions_action_check CHECK ((action = ANY (ARRAY['create'::text, 'read'::text, 'update'::text, 'delete'::text])))
);


--
-- Name: plans; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.plans (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    name character varying(100) NOT NULL,
    slug character varying(50) NOT NULL,
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


--
-- Name: projects; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.projects (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    workspace_id uuid,
    source_id uuid,
    name text NOT NULL,
    description text,
    status text DEFAULT 'active'::text NOT NULL,
    github_repo_url text,
    github_access_token text,
    created_at timestamp without time zone DEFAULT now(),
    updated_at timestamp without time zone DEFAULT now(),
    auto boolean DEFAULT false NOT NULL,
    auto_actor_id uuid,
    brief jsonb,
    auto_paused_reason text,
    auto_claimed_at timestamp with time zone,
    auto_completed_at timestamp with time zone,
    CONSTRAINT projects_status_check CHECK ((status = ANY (ARRAY['active'::text, 'on_hold'::text, 'cancelled'::text])))
);


--
-- Name: refresh_tokens; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.refresh_tokens (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    user_id uuid NOT NULL,
    token_hash text NOT NULL,
    expires_at timestamp without time zone NOT NULL,
    created_at timestamp without time zone DEFAULT now(),
    revoked_at timestamp without time zone,
    user_agent text,
    ip_address text
);


--
-- Name: reminder_turns; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.reminder_turns (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    reminder_id uuid NOT NULL,
    workspace_id uuid NOT NULL,
    chat_id uuid NOT NULL,
    created_by uuid NOT NULL,
    prompt text NOT NULL,
    attempts integer DEFAULT 0 NOT NULL,
    claimed_at timestamp with time zone,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT reminder_turns_prompt_check CHECK ((length(TRIM(BOTH FROM prompt)) > 0))
);


--
-- Name: reminders; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.reminders (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    workspace_id uuid NOT NULL,
    chat_id uuid NOT NULL,
    created_by uuid NOT NULL,
    content text NOT NULL,
    due_at timestamp with time zone NOT NULL,
    status text DEFAULT 'pending'::text NOT NULL,
    message_id uuid,
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
    CONSTRAINT reminders_content_check CHECK ((length(TRIM(BOTH FROM content)) > 0)),
    CONSTRAINT reminders_last_observation_check CHECK (((last_observation IS NULL) OR (length(last_observation) <= 4000))),
    CONSTRAINT reminders_status_check CHECK ((status = ANY (ARRAY['pending'::text, 'delivered'::text, 'cancelled'::text, 'expired'::text]))),
    CONSTRAINT reminders_timing_mode_check CHECK ((timing_mode = ANY (ARRAY['exact_schedule'::text, 'condition_watch'::text]))),
    CONSTRAINT reminders_watch_compares_check CHECK (((timing_mode <> 'condition_watch'::text) OR ((COALESCE(rrule, ''::text) ~ '[^[:space:]]'::text) AND (COALESCE(prompt, ''::text) ~ '[^[:space:]]'::text))))
);


--
-- Name: role_permissions; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.role_permissions (
    role_id uuid NOT NULL,
    permission_id uuid NOT NULL
);


--
-- Name: roles; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.roles (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    name text NOT NULL,
    description text,
    is_system boolean DEFAULT false,
    created_at timestamp without time zone DEFAULT now(),
    updated_at timestamp without time zone DEFAULT now()
);


--
-- Name: sessions; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.sessions (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    user_id uuid NOT NULL,
    refresh_token_hash character varying(255) NOT NULL,
    ip_address inet,
    user_agent text,
    device_info jsonb,
    last_active_at timestamp with time zone DEFAULT now() NOT NULL,
    expires_at timestamp with time zone NOT NULL,
    revoked_at timestamp with time zone,
    created_at timestamp with time zone DEFAULT now() NOT NULL
);


--
-- Name: source_sync_state; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.source_sync_state (
    source_id uuid NOT NULL,
    last_sync_at timestamp without time zone,
    cursor text,
    etag text,
    version text,
    extra jsonb DEFAULT '{}'::jsonb NOT NULL,
    created_at timestamp without time zone DEFAULT now(),
    updated_at timestamp without time zone DEFAULT now()
);


--
-- Name: source_types; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.source_types (
    name text NOT NULL,
    category text DEFAULT 'file'::text NOT NULL,
    description text NOT NULL,
    config_schema jsonb DEFAULT '{}'::jsonb NOT NULL,
    created_at timestamp without time zone DEFAULT now()
);


--
-- Name: sources; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.sources (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    workspace_id uuid,
    name text NOT NULL,
    source_type text NOT NULL,
    config jsonb DEFAULT '{}'::jsonb NOT NULL,
    credentials_encrypted text,
    description text,
    url text,
    is_active boolean DEFAULT true,
    last_verified_at timestamp without time zone,
    last_error text,
    created_at timestamp without time zone DEFAULT now(),
    updated_at timestamp without time zone DEFAULT now()
);


--
-- Name: subscriptions; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.subscriptions (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    organization_id uuid NOT NULL,
    plan_id uuid NOT NULL,
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


--
-- Name: sync_configs; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.sync_configs (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    project_id uuid NOT NULL,
    provider text NOT NULL,
    enabled boolean DEFAULT true NOT NULL,
    config jsonb DEFAULT '{}'::jsonb NOT NULL,
    webhook_secret_encrypted text,
    created_at timestamp without time zone DEFAULT now(),
    updated_at timestamp without time zone DEFAULT now(),
    CONSTRAINT sync_configs_provider_check CHECK ((provider = ANY (ARRAY['github'::text, 'linear'::text])))
);


--
-- Name: sync_deliveries; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.sync_deliveries (
    sync_config_id uuid NOT NULL,
    delivery_id text NOT NULL,
    received_at timestamp with time zone DEFAULT now() NOT NULL
);


--
-- Name: sync_events; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.sync_events (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    sync_config_id uuid NOT NULL,
    synced_item_id uuid,
    event_type text NOT NULL,
    direction text NOT NULL,
    payload jsonb,
    error_message text,
    created_at timestamp without time zone DEFAULT now(),
    CONSTRAINT sync_events_direction_check CHECK ((direction = ANY (ARRAY['outbound'::text, 'inbound'::text]))),
    CONSTRAINT sync_events_event_type_check CHECK ((event_type = ANY (ARRAY['create'::text, 'update'::text, 'close'::text, 'unlink'::text, 'webhook_received'::text, 'sync_error'::text])))
);


--
-- Name: sync_unlinked_items; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.sync_unlinked_items (
    sync_config_id uuid NOT NULL,
    external_id text NOT NULL,
    unlinked_at timestamp with time zone DEFAULT now() NOT NULL
);


--
-- Name: synced_items; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.synced_items (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    sync_config_id uuid NOT NULL,
    task_id uuid NOT NULL,
    external_id text NOT NULL,
    external_url text,
    last_synced_at timestamp without time zone DEFAULT now(),
    sync_direction text DEFAULT 'bidirectional'::text NOT NULL,
    last_external_state jsonb,
    created_at timestamp without time zone DEFAULT now(),
    CONSTRAINT synced_items_sync_direction_check CHECK ((sync_direction = ANY (ARRAY['outbound'::text, 'inbound'::text, 'bidirectional'::text])))
);


--
-- Name: task_automation; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.task_automation (
    task_id uuid NOT NULL,
    project_id uuid NOT NULL,
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
    last_run_id uuid,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT task_automation_kind_check CHECK ((kind = ANY (ARRAY['scaffold'::text, 'ci'::text, 'tests'::text, 'feature'::text, 'deployment'::text, 'docs'::text, 'fix'::text]))),
    CONSTRAINT task_automation_stage_check CHECK ((stage = ANY (ARRAY['idle'::text, 'running'::text, 'no_changes'::text, 'awaiting_checks'::text, 'awaiting_reviews'::text, 'fixing'::text, 'merging'::text, 'post_merge'::text, 'merged'::text, 'paused'::text])))
);


--
-- Name: task_file_changes; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.task_file_changes (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    task_run_id uuid NOT NULL,
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


--
-- Name: task_projects; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.task_projects (
    task_id uuid NOT NULL,
    project_id uuid NOT NULL,
    created_at timestamp without time zone DEFAULT now()
);


--
-- Name: task_queue; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.task_queue (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    task_id uuid NOT NULL,
    priority integer DEFAULT 3 NOT NULL,
    queued_at timestamp without time zone DEFAULT now(),
    started_at timestamp without time zone,
    worker_id text,
    attempts integer DEFAULT 0,
    max_attempts integer DEFAULT 3,
    last_error text
);


--
-- Name: task_reviews; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.task_reviews (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    task_id uuid NOT NULL,
    run_id uuid,
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
    CONSTRAINT task_reviews_verdict_check CHECK ((verdict = ANY (ARRAY['approve'::text, 'request_changes'::text, 'unparseable'::text, 'failed'::text])))
);


--
-- Name: task_run_checkouts; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.task_run_checkouts (
    run_id uuid NOT NULL,
    head text NOT NULL,
    published text,
    updated_at timestamp with time zone DEFAULT now() NOT NULL
);


--
-- Name: task_run_logs; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.task_run_logs (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    task_run_id uuid NOT NULL,
    phase text NOT NULL,
    agent_type text NOT NULL,
    log_level text NOT NULL,
    message text NOT NULL,
    metadata jsonb DEFAULT '{}'::jsonb,
    created_at timestamp without time zone DEFAULT now(),
    CONSTRAINT task_run_logs_log_level_check CHECK ((log_level = ANY (ARRAY['debug'::text, 'info'::text, 'warning'::text, 'error'::text])))
);


--
-- Name: task_runs; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.task_runs (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
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
    triggered_by uuid,
    pending_question jsonb,
    pending_wait jsonb,
    plan text,
    model text,
    unattended boolean DEFAULT false NOT NULL,
    CONSTRAINT task_runs_progress_percent_check CHECK (((progress_percent >= 0) AND (progress_percent <= 100))),
    CONSTRAINT task_runs_status_check CHECK ((status = ANY (ARRAY['running'::text, 'waiting'::text, 'completed'::text, 'failed'::text, 'cancelled'::text])))
);


--
-- Name: task_tool_calls; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.task_tool_calls (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    task_run_id uuid NOT NULL,
    tool_name text NOT NULL,
    tool_input jsonb NOT NULL,
    tool_output jsonb,
    status text DEFAULT 'pending'::text NOT NULL,
    error_message text,
    started_at timestamp without time zone DEFAULT now(),
    completed_at timestamp without time zone,
    CONSTRAINT task_tool_calls_status_check CHECK ((status = ANY (ARRAY['pending'::text, 'running'::text, 'completed'::text, 'failed'::text])))
);


--
-- Name: tasks; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.tasks (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    workspace_id uuid NOT NULL,
    title text NOT NULL,
    description text NOT NULL,
    acceptance_criteria text,
    status text DEFAULT 'created'::text NOT NULL,
    priority integer,
    model_name text,
    dependencies jsonb DEFAULT '[]'::jsonb,
    is_agentic boolean DEFAULT false NOT NULL,
    github_repo_url text,
    source_id uuid,
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
    assignee_id uuid,
    created_by uuid,
    active_run_id uuid,
    require_plan_approval boolean DEFAULT false NOT NULL,
    CONSTRAINT tasks_priority_check CHECK (((priority >= 1) AND (priority <= 5))),
    CONSTRAINT tasks_status_check CHECK ((status = ANY (ARRAY['created'::text, 'queued'::text, 'in_progress'::text, 'review'::text, 'complete'::text, 'blocked'::text])))
);


--
-- Name: usage_events; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.usage_events (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    organization_id uuid NOT NULL,
    workspace_id uuid,
    user_id uuid,
    event_type character varying(50) NOT NULL,
    quantity bigint DEFAULT 1 NOT NULL,
    metadata jsonb DEFAULT '{}'::jsonb,
    recorded_at timestamp with time zone DEFAULT now() NOT NULL
);


--
-- Name: user_roles; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.user_roles (
    user_id uuid NOT NULL,
    role_id uuid NOT NULL,
    assigned_at timestamp without time zone DEFAULT now(),
    assigned_by uuid
);


--
-- Name: users; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.users (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    email text NOT NULL,
    password_hash text NOT NULL,
    display_name text,
    is_active boolean DEFAULT true,
    is_admin boolean DEFAULT false,
    email_verified boolean DEFAULT false NOT NULL,
    created_at timestamp without time zone DEFAULT now(),
    updated_at timestamp without time zone DEFAULT now(),
    last_login_at timestamp without time zone
);


--
-- Name: wiki_chunks; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.wiki_chunks (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    wiki_entry_id uuid NOT NULL,
    chunk_index integer NOT NULL,
    content text NOT NULL,
    embedding public.vector(1024),
    token_count integer,
    created_at timestamp without time zone DEFAULT now()
);


--
-- Name: wiki_entries; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.wiki_entries (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
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


--
-- Name: workspace_ai_settings; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.workspace_ai_settings (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    workspace_id uuid NOT NULL,
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
    CONSTRAINT workspace_ai_settings_provider_check CHECK (((provider IS NULL) OR (provider = ANY (ARRAY['self_hosted'::text, 'openai'::text, 'anthropic'::text, 'bedrock'::text, 'claude_code'::text, 'codex'::text]))))
);


--
-- Name: workspace_members; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.workspace_members (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    workspace_id uuid NOT NULL,
    user_id uuid NOT NULL,
    role text DEFAULT 'member'::text NOT NULL,
    is_active boolean DEFAULT true NOT NULL,
    invited_by uuid,
    invited_at timestamp without time zone,
    accepted_at timestamp without time zone,
    created_at timestamp without time zone DEFAULT now(),
    updated_at timestamp without time zone DEFAULT now(),
    CONSTRAINT workspace_members_role_check CHECK ((role = ANY (ARRAY['owner'::text, 'admin'::text, 'member'::text, 'viewer'::text])))
);


--
-- Name: workspace_themes; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.workspace_themes (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    workspace_id uuid NOT NULL,
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


--
-- Name: workspaces; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.workspaces (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    organization_id uuid NOT NULL,
    name text NOT NULL,
    slug text NOT NULL,
    description text,
    is_active boolean DEFAULT true,
    created_at timestamp without time zone DEFAULT now(),
    updated_at timestamp without time zone DEFAULT now()
);


--
-- Name: agent_logins agent_logins_organization_id_agent_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.agent_logins
    ADD CONSTRAINT agent_logins_organization_id_agent_key UNIQUE (organization_id, agent);


--
-- Name: agent_logins agent_logins_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.agent_logins
    ADD CONSTRAINT agent_logins_pkey PRIMARY KEY (id);


--
-- Name: audit_logs audit_logs_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.audit_logs
    ADD CONSTRAINT audit_logs_pkey PRIMARY KEY (id);


--
-- Name: chat_attached_sources chat_attached_sources_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.chat_attached_sources
    ADD CONSTRAINT chat_attached_sources_pkey PRIMARY KEY (chat_id, source_id);


--
-- Name: chat_calls chat_calls_chat_id_result_id_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.chat_calls
    ADD CONSTRAINT chat_calls_chat_id_result_id_key UNIQUE (chat_id, result_id);


--
-- Name: chat_calls chat_calls_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.chat_calls
    ADD CONSTRAINT chat_calls_pkey PRIMARY KEY (chat_id, id);


--
-- Name: chat_checkpoints chat_checkpoints_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.chat_checkpoints
    ADD CONSTRAINT chat_checkpoints_pkey PRIMARY KEY (chat_id);


--
-- Name: chat_entries chat_entries_chat_id_position_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.chat_entries
    ADD CONSTRAINT chat_entries_chat_id_position_key UNIQUE (chat_id, "position");


--
-- Name: chat_entries chat_entries_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.chat_entries
    ADD CONSTRAINT chat_entries_pkey PRIMARY KEY (chat_id, id);


--
-- Name: chat_leases chat_leases_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.chat_leases
    ADD CONSTRAINT chat_leases_pkey PRIMARY KEY (chat_id);


--
-- Name: chat_sources chat_sources_identity; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.chat_sources
    ADD CONSTRAINT chat_sources_identity UNIQUE (chat_id, kind, key);


--
-- Name: chat_sources chat_sources_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.chat_sources
    ADD CONSTRAINT chat_sources_pkey PRIMARY KEY (chat_id, identifier);


--
-- Name: chat_turns chat_turns_chat_id_id_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.chat_turns
    ADD CONSTRAINT chat_turns_chat_id_id_key UNIQUE (chat_id, id);


--
-- Name: chat_turns chat_turns_chat_id_user_message_id_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.chat_turns
    ADD CONSTRAINT chat_turns_chat_id_user_message_id_key UNIQUE (chat_id, user_message_id);


--
-- Name: chat_turns chat_turns_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.chat_turns
    ADD CONSTRAINT chat_turns_pkey PRIMARY KEY (id);


--
-- Name: chats chats_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.chats
    ADD CONSTRAINT chats_pkey PRIMARY KEY (id);


--
-- Name: content_chunks content_chunks_content_item_id_chunk_index_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.content_chunks
    ADD CONSTRAINT content_chunks_content_item_id_chunk_index_key UNIQUE (content_item_id, chunk_index);


--
-- Name: content_chunks content_chunks_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.content_chunks
    ADD CONSTRAINT content_chunks_pkey PRIMARY KEY (id);


--
-- Name: content_items content_items_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.content_items
    ADD CONSTRAINT content_items_pkey PRIMARY KEY (id);


--
-- Name: content_items content_items_source_id_uri_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.content_items
    ADD CONSTRAINT content_items_source_id_uri_key UNIQUE (source_id, uri);


--
-- Name: context_gatherings context_gatherings_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.context_gatherings
    ADD CONSTRAINT context_gatherings_pkey PRIMARY KEY (id);


--
-- Name: email_verification_tokens email_verification_tokens_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.email_verification_tokens
    ADD CONSTRAINT email_verification_tokens_pkey PRIMARY KEY (id);


--
-- Name: email_verification_tokens email_verification_tokens_token_hash_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.email_verification_tokens
    ADD CONSTRAINT email_verification_tokens_token_hash_key UNIQUE (token_hash);


--
-- Name: embeddings embeddings_chunk_id_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.embeddings
    ADD CONSTRAINT embeddings_chunk_id_key UNIQUE (chunk_id);


--
-- Name: embeddings embeddings_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.embeddings
    ADD CONSTRAINT embeddings_pkey PRIMARY KEY (id);


--
-- Name: gathering_events gathering_events_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.gathering_events
    ADD CONSTRAINT gathering_events_pkey PRIMARY KEY (id);


--
-- Name: heuristic_analysis heuristic_analysis_content_item_id_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.heuristic_analysis
    ADD CONSTRAINT heuristic_analysis_content_item_id_key UNIQUE (content_item_id);


--
-- Name: heuristic_analysis heuristic_analysis_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.heuristic_analysis
    ADD CONSTRAINT heuristic_analysis_pkey PRIMARY KEY (id);


--
-- Name: invitations invitations_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.invitations
    ADD CONSTRAINT invitations_pkey PRIMARY KEY (id);


--
-- Name: invitations invitations_token_hash_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.invitations
    ADD CONSTRAINT invitations_token_hash_key UNIQUE (token_hash);


--
-- Name: knowledge_embeddings knowledge_embeddings_knowledge_entry_id_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.knowledge_embeddings
    ADD CONSTRAINT knowledge_embeddings_knowledge_entry_id_key UNIQUE (knowledge_entry_id);


--
-- Name: knowledge_embeddings knowledge_embeddings_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.knowledge_embeddings
    ADD CONSTRAINT knowledge_embeddings_pkey PRIMARY KEY (id);


--
-- Name: knowledge_entries knowledge_entries_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.knowledge_entries
    ADD CONSTRAINT knowledge_entries_pkey PRIMARY KEY (id);


--
-- Name: message_embeddings message_embeddings_message_id_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.message_embeddings
    ADD CONSTRAINT message_embeddings_message_id_key UNIQUE (message_id);


--
-- Name: message_embeddings message_embeddings_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.message_embeddings
    ADD CONSTRAINT message_embeddings_pkey PRIMARY KEY (id);


--
-- Name: messages messages_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.messages
    ADD CONSTRAINT messages_pkey PRIMARY KEY (id);


--
-- Name: organization_ai_settings organization_ai_settings_organization_id_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.organization_ai_settings
    ADD CONSTRAINT organization_ai_settings_organization_id_key UNIQUE (organization_id);


--
-- Name: organization_ai_settings organization_ai_settings_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.organization_ai_settings
    ADD CONSTRAINT organization_ai_settings_pkey PRIMARY KEY (id);


--
-- Name: organization_members organization_members_organization_id_user_id_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.organization_members
    ADD CONSTRAINT organization_members_organization_id_user_id_key UNIQUE (organization_id, user_id);


--
-- Name: organization_members organization_members_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.organization_members
    ADD CONSTRAINT organization_members_pkey PRIMARY KEY (id);


--
-- Name: organizations organizations_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.organizations
    ADD CONSTRAINT organizations_pkey PRIMARY KEY (id);


--
-- Name: organizations organizations_slug_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.organizations
    ADD CONSTRAINT organizations_slug_key UNIQUE (slug);


--
-- Name: password_reset_tokens password_reset_tokens_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.password_reset_tokens
    ADD CONSTRAINT password_reset_tokens_pkey PRIMARY KEY (id);


--
-- Name: password_reset_tokens password_reset_tokens_token_hash_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.password_reset_tokens
    ADD CONSTRAINT password_reset_tokens_token_hash_key UNIQUE (token_hash);


--
-- Name: permissions permissions_name_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.permissions
    ADD CONSTRAINT permissions_name_key UNIQUE (name);


--
-- Name: permissions permissions_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.permissions
    ADD CONSTRAINT permissions_pkey PRIMARY KEY (id);


--
-- Name: plans plans_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.plans
    ADD CONSTRAINT plans_pkey PRIMARY KEY (id);


--
-- Name: plans plans_slug_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.plans
    ADD CONSTRAINT plans_slug_key UNIQUE (slug);


--
-- Name: projects projects_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.projects
    ADD CONSTRAINT projects_pkey PRIMARY KEY (id);


--
-- Name: refresh_tokens refresh_tokens_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.refresh_tokens
    ADD CONSTRAINT refresh_tokens_pkey PRIMARY KEY (id);


--
-- Name: refresh_tokens refresh_tokens_token_hash_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.refresh_tokens
    ADD CONSTRAINT refresh_tokens_token_hash_key UNIQUE (token_hash);


--
-- Name: reminder_turns reminder_turns_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.reminder_turns
    ADD CONSTRAINT reminder_turns_pkey PRIMARY KEY (id);


--
-- Name: reminders reminders_message_id_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.reminders
    ADD CONSTRAINT reminders_message_id_key UNIQUE (message_id);


--
-- Name: reminders reminders_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.reminders
    ADD CONSTRAINT reminders_pkey PRIMARY KEY (id);


--
-- Name: role_permissions role_permissions_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.role_permissions
    ADD CONSTRAINT role_permissions_pkey PRIMARY KEY (role_id, permission_id);


--
-- Name: roles roles_name_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.roles
    ADD CONSTRAINT roles_name_key UNIQUE (name);


--
-- Name: roles roles_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.roles
    ADD CONSTRAINT roles_pkey PRIMARY KEY (id);


--
-- Name: sessions sessions_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.sessions
    ADD CONSTRAINT sessions_pkey PRIMARY KEY (id);


--
-- Name: sessions sessions_refresh_token_hash_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.sessions
    ADD CONSTRAINT sessions_refresh_token_hash_key UNIQUE (refresh_token_hash);


--
-- Name: source_sync_state source_sync_state_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.source_sync_state
    ADD CONSTRAINT source_sync_state_pkey PRIMARY KEY (source_id);


--
-- Name: source_types source_types_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.source_types
    ADD CONSTRAINT source_types_pkey PRIMARY KEY (name);


--
-- Name: sources sources_name_source_type_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.sources
    ADD CONSTRAINT sources_name_source_type_key UNIQUE (name, source_type);


--
-- Name: sources sources_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.sources
    ADD CONSTRAINT sources_pkey PRIMARY KEY (id);


--
-- Name: subscriptions subscriptions_organization_id_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.subscriptions
    ADD CONSTRAINT subscriptions_organization_id_key UNIQUE (organization_id);


--
-- Name: subscriptions subscriptions_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.subscriptions
    ADD CONSTRAINT subscriptions_pkey PRIMARY KEY (id);


--
-- Name: sync_configs sync_configs_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.sync_configs
    ADD CONSTRAINT sync_configs_pkey PRIMARY KEY (id);


--
-- Name: sync_configs sync_configs_project_id_provider_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.sync_configs
    ADD CONSTRAINT sync_configs_project_id_provider_key UNIQUE (project_id, provider);


--
-- Name: sync_deliveries sync_deliveries_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.sync_deliveries
    ADD CONSTRAINT sync_deliveries_pkey PRIMARY KEY (sync_config_id, delivery_id);


--
-- Name: sync_events sync_events_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.sync_events
    ADD CONSTRAINT sync_events_pkey PRIMARY KEY (id);


--
-- Name: sync_unlinked_items sync_unlinked_items_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.sync_unlinked_items
    ADD CONSTRAINT sync_unlinked_items_pkey PRIMARY KEY (sync_config_id, external_id);


--
-- Name: synced_items synced_items_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.synced_items
    ADD CONSTRAINT synced_items_pkey PRIMARY KEY (id);


--
-- Name: synced_items synced_items_sync_config_id_external_id_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.synced_items
    ADD CONSTRAINT synced_items_sync_config_id_external_id_key UNIQUE (sync_config_id, external_id);


--
-- Name: synced_items synced_items_sync_config_id_task_id_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.synced_items
    ADD CONSTRAINT synced_items_sync_config_id_task_id_key UNIQUE (sync_config_id, task_id);


--
-- Name: task_automation task_automation_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.task_automation
    ADD CONSTRAINT task_automation_pkey PRIMARY KEY (task_id);


--
-- Name: task_file_changes task_file_changes_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.task_file_changes
    ADD CONSTRAINT task_file_changes_pkey PRIMARY KEY (id);


--
-- Name: task_projects task_projects_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.task_projects
    ADD CONSTRAINT task_projects_pkey PRIMARY KEY (task_id, project_id);


--
-- Name: task_queue task_queue_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.task_queue
    ADD CONSTRAINT task_queue_pkey PRIMARY KEY (id);


--
-- Name: task_queue task_queue_task_id_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.task_queue
    ADD CONSTRAINT task_queue_task_id_key UNIQUE (task_id);


--
-- Name: task_reviews task_reviews_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.task_reviews
    ADD CONSTRAINT task_reviews_pkey PRIMARY KEY (id);


--
-- Name: task_reviews task_reviews_task_id_round_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.task_reviews
    ADD CONSTRAINT task_reviews_task_id_round_key UNIQUE (task_id, round);


--
-- Name: task_run_checkouts task_run_checkouts_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.task_run_checkouts
    ADD CONSTRAINT task_run_checkouts_pkey PRIMARY KEY (run_id);


--
-- Name: task_run_logs task_run_logs_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.task_run_logs
    ADD CONSTRAINT task_run_logs_pkey PRIMARY KEY (id);


--
-- Name: task_runs task_runs_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.task_runs
    ADD CONSTRAINT task_runs_pkey PRIMARY KEY (id);


--
-- Name: task_tool_calls task_tool_calls_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.task_tool_calls
    ADD CONSTRAINT task_tool_calls_pkey PRIMARY KEY (id);


--
-- Name: tasks tasks_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.tasks
    ADD CONSTRAINT tasks_pkey PRIMARY KEY (id);


--
-- Name: usage_events usage_events_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.usage_events
    ADD CONSTRAINT usage_events_pkey PRIMARY KEY (id);


--
-- Name: user_roles user_roles_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.user_roles
    ADD CONSTRAINT user_roles_pkey PRIMARY KEY (user_id, role_id);


--
-- Name: users users_email_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.users
    ADD CONSTRAINT users_email_key UNIQUE (email);


--
-- Name: users users_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.users
    ADD CONSTRAINT users_pkey PRIMARY KEY (id);


--
-- Name: wiki_chunks wiki_chunks_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.wiki_chunks
    ADD CONSTRAINT wiki_chunks_pkey PRIMARY KEY (id);


--
-- Name: wiki_entries wiki_entries_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.wiki_entries
    ADD CONSTRAINT wiki_entries_pkey PRIMARY KEY (id);


--
-- Name: workspace_ai_settings workspace_ai_settings_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workspace_ai_settings
    ADD CONSTRAINT workspace_ai_settings_pkey PRIMARY KEY (id);


--
-- Name: workspace_ai_settings workspace_ai_settings_workspace_id_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workspace_ai_settings
    ADD CONSTRAINT workspace_ai_settings_workspace_id_key UNIQUE (workspace_id);


--
-- Name: workspace_members workspace_members_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workspace_members
    ADD CONSTRAINT workspace_members_pkey PRIMARY KEY (id);


--
-- Name: workspace_members workspace_members_workspace_id_user_id_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workspace_members
    ADD CONSTRAINT workspace_members_workspace_id_user_id_key UNIQUE (workspace_id, user_id);


--
-- Name: workspace_themes workspace_themes_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workspace_themes
    ADD CONSTRAINT workspace_themes_pkey PRIMARY KEY (id);


--
-- Name: workspace_themes workspace_themes_workspace_id_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workspace_themes
    ADD CONSTRAINT workspace_themes_workspace_id_key UNIQUE (workspace_id);


--
-- Name: workspaces workspaces_organization_id_slug_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workspaces
    ADD CONSTRAINT workspaces_organization_id_slug_key UNIQUE (organization_id, slug);


--
-- Name: workspaces workspaces_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workspaces
    ADD CONSTRAINT workspaces_pkey PRIMARY KEY (id);


--
-- Name: chat_calls_pending; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX chat_calls_pending ON public.chat_calls USING btree (chat_id, turn_id) WHERE (result_id IS NULL);


--
-- Name: chat_turns_running; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX chat_turns_running ON public.chat_turns USING btree (chat_id) WHERE (status = 'running'::text);


--
-- Name: idx_audit_logs_action; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_audit_logs_action ON public.audit_logs USING btree (action);


--
-- Name: idx_audit_logs_actor; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_audit_logs_actor ON public.audit_logs USING btree (actor_id);


--
-- Name: idx_audit_logs_created; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_audit_logs_created ON public.audit_logs USING btree (created_at DESC);


--
-- Name: idx_audit_logs_org; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_audit_logs_org ON public.audit_logs USING btree (organization_id, created_at DESC);


--
-- Name: idx_audit_logs_resource; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_audit_logs_resource ON public.audit_logs USING btree (resource_type, resource_id);


--
-- Name: idx_chat_attached_sources_source; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_chat_attached_sources_source ON public.chat_attached_sources USING btree (source_id);


--
-- Name: idx_chats_project; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_chats_project ON public.chats USING btree (project_id) WHERE (project_id IS NOT NULL);


--
-- Name: idx_chats_workspace_archived; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_chats_workspace_archived ON public.chats USING btree (workspace_id, archived, updated_at DESC);


--
-- Name: idx_content_chunks_item; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_content_chunks_item ON public.content_chunks USING btree (content_item_id);


--
-- Name: idx_content_chunks_search; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_content_chunks_search ON public.content_chunks USING gin (search_vector);


--
-- Name: idx_content_documents_search; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_content_documents_search ON public.content_items USING gin (to_tsvector('english'::regconfig, ((title || ' '::text) || COALESCE(
CASE
    WHEN metadata_only THEN NULL::text
    ELSE content
END, ''::text))));


--
-- Name: idx_content_items_category; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_content_items_category ON public.content_items USING btree (category);


--
-- Name: idx_content_items_fetched; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_content_items_fetched ON public.content_items USING btree (fetched_at);


--
-- Name: idx_content_items_hash; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_content_items_hash ON public.content_items USING btree (content_hash);


--
-- Name: idx_content_items_metadata; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_content_items_metadata ON public.content_items USING gin (metadata);


--
-- Name: idx_content_items_search; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_content_items_search ON public.content_items USING gin (search_vector);


--
-- Name: idx_content_items_source; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_content_items_source ON public.content_items USING btree (source_id);


--
-- Name: idx_content_items_workspace; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_content_items_workspace ON public.content_items USING btree (workspace_id);


--
-- Name: idx_context_gatherings_source_ids; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_context_gatherings_source_ids ON public.context_gatherings USING gin (source_ids);


--
-- Name: idx_context_gatherings_status; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_context_gatherings_status ON public.context_gatherings USING btree (status);


--
-- Name: idx_context_gatherings_task; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_context_gatherings_task ON public.context_gatherings USING btree (task_id);


--
-- Name: idx_context_gatherings_user; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_context_gatherings_user ON public.context_gatherings USING btree (user_id);


--
-- Name: idx_context_gatherings_workspace; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_context_gatherings_workspace ON public.context_gatherings USING btree (workspace_id);


--
-- Name: idx_email_verification_tokens_user; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_email_verification_tokens_user ON public.email_verification_tokens USING btree (user_id);


--
-- Name: idx_embeddings_content_item; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_embeddings_content_item ON public.embeddings USING btree (content_item_id);


--
-- Name: idx_embeddings_source; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_embeddings_source ON public.embeddings USING btree (source_id);


--
-- Name: idx_embeddings_vector; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_embeddings_vector ON public.embeddings USING hnsw (vector public.vector_cosine_ops) WITH (m='16', ef_construction='64');


--
-- Name: idx_embeddings_vector_bit; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_embeddings_vector_bit ON public.embeddings USING hnsw (vector_bit public.bit_hamming_ops) WITH (m='16', ef_construction='64');


--
-- Name: idx_embeddings_workspace; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_embeddings_workspace ON public.embeddings USING btree (workspace_id);


--
-- Name: idx_embeddings_workspace_source; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_embeddings_workspace_source ON public.embeddings USING btree (workspace_id, source_id);


--
-- Name: idx_gathering_events_created; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_gathering_events_created ON public.gathering_events USING btree (created_at);


--
-- Name: idx_gathering_events_gathering; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_gathering_events_gathering ON public.gathering_events USING btree (gathering_id);


--
-- Name: idx_gathering_events_gathering_created; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_gathering_events_gathering_created ON public.gathering_events USING btree (gathering_id, created_at);


--
-- Name: idx_heuristic_analysis_categorization; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_heuristic_analysis_categorization ON public.heuristic_analysis USING gin (categorization);


--
-- Name: idx_heuristic_analysis_entities; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_heuristic_analysis_entities ON public.heuristic_analysis USING gin (entities);


--
-- Name: idx_heuristic_analysis_item; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_heuristic_analysis_item ON public.heuristic_analysis USING btree (content_item_id);


--
-- Name: idx_invitations_org; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_invitations_org ON public.invitations USING btree (organization_id);


--
-- Name: idx_invitations_pending; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_invitations_pending ON public.invitations USING btree (email, organization_id, expires_at) WHERE (accepted_at IS NULL);


--
-- Name: idx_invitations_token; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_invitations_token ON public.invitations USING btree (token_hash);


--
-- Name: idx_knowledge_documents_search; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_knowledge_documents_search ON public.knowledge_entries USING gin (to_tsvector('english'::regconfig, ((title || ' '::text) || COALESCE(content, ''::text)))) WHERE (is_active = true);


--
-- Name: idx_knowledge_embeddings_vector; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_knowledge_embeddings_vector ON public.knowledge_embeddings USING hnsw (vector public.vector_cosine_ops) WITH (m='16', ef_construction='64');


--
-- Name: idx_knowledge_embeddings_vector_bit; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_knowledge_embeddings_vector_bit ON public.knowledge_embeddings USING hnsw (vector_bit public.bit_hamming_ops) WITH (m='16', ef_construction='64');


--
-- Name: idx_knowledge_embeddings_workspace; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_knowledge_embeddings_workspace ON public.knowledge_embeddings USING btree (workspace_id);


--
-- Name: idx_knowledge_entries_search_vector; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_knowledge_entries_search_vector ON public.knowledge_entries USING gin (search_vector) WHERE (is_active = true);


--
-- Name: idx_knowledge_entries_tags; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_knowledge_entries_tags ON public.knowledge_entries USING gin (tags);


--
-- Name: idx_knowledge_memory_entry; Type: INDEX; Schema: public; Owner: -
--

CREATE UNIQUE INDEX idx_knowledge_memory_entry ON public.knowledge_entries USING btree (workspace_id, created_by, category, title) WHERE ((is_active = true) AND (category ~~ 'memory-%'::text));


--
-- Name: idx_knowledge_refresh_due; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_knowledge_refresh_due ON public.knowledge_entries USING btree (workspace_id, source_url) WHERE ((source_url IS NOT NULL) AND (is_active = true));


--
-- Name: idx_knowledge_source_url; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_knowledge_source_url ON public.knowledge_entries USING btree (workspace_id, source_url) WHERE (source_url IS NOT NULL);


--
-- Name: idx_knowledge_workspace_category; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_knowledge_workspace_category ON public.knowledge_entries USING btree (workspace_id, category, created_at DESC) WHERE (is_active = true);


--
-- Name: idx_message_embeddings_chat; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_message_embeddings_chat ON public.message_embeddings USING btree (chat_id);


--
-- Name: idx_message_embeddings_vector; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_message_embeddings_vector ON public.message_embeddings USING hnsw (vector public.vector_cosine_ops) WITH (m='16', ef_construction='64');


--
-- Name: idx_message_embeddings_vector_bit; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_message_embeddings_vector_bit ON public.message_embeddings USING hnsw (vector_bit public.bit_hamming_ops) WITH (m='16', ef_construction='64');


--
-- Name: idx_message_embeddings_workspace_chat; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_message_embeddings_workspace_chat ON public.message_embeddings USING btree (workspace_id, chat_id);


--
-- Name: idx_messages_chat_created; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_messages_chat_created ON public.messages USING btree (chat_id, created_at);


--
-- Name: idx_messages_search_vector; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_messages_search_vector ON public.messages USING gin (search_vector);


--
-- Name: idx_org_ai_settings_org_id; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_org_ai_settings_org_id ON public.organization_ai_settings USING btree (organization_id);


--
-- Name: idx_org_members_active; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_org_members_active ON public.organization_members USING btree (organization_id, is_active) WHERE (is_active = true);


--
-- Name: idx_org_members_org; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_org_members_org ON public.organization_members USING btree (organization_id);


--
-- Name: idx_org_members_user_active; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_org_members_user_active ON public.organization_members USING btree (user_id) WHERE (is_active = true);


--
-- Name: idx_organizations_active; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_organizations_active ON public.organizations USING btree (is_active) WHERE (is_active = true);


--
-- Name: idx_password_reset_tokens_user; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_password_reset_tokens_user ON public.password_reset_tokens USING btree (user_id);


--
-- Name: idx_permissions_resource; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_permissions_resource ON public.permissions USING btree (resource);


--
-- Name: idx_projects_auto; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_projects_auto ON public.projects USING btree (id) WHERE auto;


--
-- Name: idx_projects_status; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_projects_status ON public.projects USING btree (status);


--
-- Name: idx_projects_workspace_status; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_projects_workspace_status ON public.projects USING btree (workspace_id, status, created_at DESC);


--
-- Name: idx_refresh_tokens_expires_at; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_refresh_tokens_expires_at ON public.refresh_tokens USING btree (expires_at);


--
-- Name: idx_refresh_tokens_user_id; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_refresh_tokens_user_id ON public.refresh_tokens USING btree (user_id);


--
-- Name: idx_role_permissions_role_id; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_role_permissions_role_id ON public.role_permissions USING btree (role_id);


--
-- Name: idx_sessions_expires; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_sessions_expires ON public.sessions USING btree (expires_at);


--
-- Name: idx_sessions_user_active; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_sessions_user_active ON public.sessions USING btree (user_id, last_active_at DESC) WHERE (revoked_at IS NULL);


--
-- Name: idx_source_types_category; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_source_types_category ON public.source_types USING btree (category);


--
-- Name: idx_sources_active; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_sources_active ON public.sources USING btree (is_active) WHERE (is_active = true);


--
-- Name: idx_sources_workspace_filters; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_sources_workspace_filters ON public.sources USING btree (workspace_id, source_type, is_active, created_at DESC);


--
-- Name: idx_subscriptions_status; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_subscriptions_status ON public.subscriptions USING btree (status);


--
-- Name: idx_subscriptions_stripe; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_subscriptions_stripe ON public.subscriptions USING btree (stripe_subscription_id);


--
-- Name: idx_sync_configs_enabled; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_sync_configs_enabled ON public.sync_configs USING btree (enabled) WHERE (enabled = true);


--
-- Name: idx_sync_configs_project_id; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_sync_configs_project_id ON public.sync_configs USING btree (project_id);


--
-- Name: idx_sync_configs_provider; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_sync_configs_provider ON public.sync_configs USING btree (provider);


--
-- Name: idx_sync_events_created_at; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_sync_events_created_at ON public.sync_events USING btree (created_at DESC);


--
-- Name: idx_sync_events_event_type; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_sync_events_event_type ON public.sync_events USING btree (event_type);


--
-- Name: idx_sync_events_sync_config_id; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_sync_events_sync_config_id ON public.sync_events USING btree (sync_config_id);


--
-- Name: idx_sync_events_synced_item_id; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_sync_events_synced_item_id ON public.sync_events USING btree (synced_item_id);


--
-- Name: idx_sync_state_last_sync; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_sync_state_last_sync ON public.source_sync_state USING btree (last_sync_at);


--
-- Name: idx_synced_items_external_id; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_synced_items_external_id ON public.synced_items USING btree (sync_config_id, external_id);


--
-- Name: idx_synced_items_last_synced; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_synced_items_last_synced ON public.synced_items USING btree (last_synced_at);


--
-- Name: idx_synced_items_sync_config_id; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_synced_items_sync_config_id ON public.synced_items USING btree (sync_config_id);


--
-- Name: idx_synced_items_task_id; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_synced_items_task_id ON public.synced_items USING btree (task_id);


--
-- Name: idx_task_automation_project_stage; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_task_automation_project_stage ON public.task_automation USING btree (project_id, stage);


--
-- Name: idx_task_file_changes_applied; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_task_file_changes_applied ON public.task_file_changes USING btree (applied);


--
-- Name: idx_task_file_changes_run_id; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_task_file_changes_run_id ON public.task_file_changes USING btree (task_run_id);


--
-- Name: idx_task_projects_project_id; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_task_projects_project_id ON public.task_projects USING btree (project_id);


--
-- Name: idx_task_projects_task_id; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_task_projects_task_id ON public.task_projects USING btree (task_id);


--
-- Name: idx_task_queue_priority; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_task_queue_priority ON public.task_queue USING btree (priority DESC, queued_at);


--
-- Name: idx_task_queue_worker; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_task_queue_worker ON public.task_queue USING btree (worker_id) WHERE (worker_id IS NOT NULL);


--
-- Name: idx_task_run_logs_run_created; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_task_run_logs_run_created ON public.task_run_logs USING btree (task_run_id, created_at);


--
-- Name: idx_task_runs_status; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_task_runs_status ON public.task_runs USING btree (status);


--
-- Name: idx_task_runs_task_started; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_task_runs_task_started ON public.task_runs USING btree (task_id, started_at DESC);


--
-- Name: idx_task_tool_calls_run_id; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_task_tool_calls_run_id ON public.task_tool_calls USING btree (task_run_id);


--
-- Name: idx_task_tool_calls_status; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_task_tool_calls_status ON public.task_tool_calls USING btree (status);


--
-- Name: idx_tasks_branch_name; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_tasks_branch_name ON public.tasks USING btree (branch_name) WHERE (branch_name IS NOT NULL);


--
-- Name: idx_tasks_pr_status; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_tasks_pr_status ON public.tasks USING btree (pr_status) WHERE (pr_status IS NOT NULL);


--
-- Name: idx_tasks_queued_at; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_tasks_queued_at ON public.tasks USING btree (queued_at) WHERE (queued_at IS NOT NULL);


--
-- Name: idx_tasks_source_ids; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_tasks_source_ids ON public.tasks USING gin (source_ids);


--
-- Name: idx_tasks_status; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_tasks_status ON public.tasks USING btree (status);


--
-- Name: idx_tasks_worker_id; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_tasks_worker_id ON public.tasks USING btree (worker_id) WHERE (worker_id IS NOT NULL);


--
-- Name: idx_tasks_workspace_created; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_tasks_workspace_created ON public.tasks USING btree (workspace_id, created_at DESC);


--
-- Name: idx_tasks_workspace_status; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_tasks_workspace_status ON public.tasks USING btree (workspace_id, status, created_at DESC);


--
-- Name: idx_usage_events_org_time; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_usage_events_org_time ON public.usage_events USING btree (organization_id, recorded_at);


--
-- Name: idx_usage_events_org_type_time; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_usage_events_org_type_time ON public.usage_events USING btree (organization_id, event_type, recorded_at);


--
-- Name: idx_usage_events_type; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_usage_events_type ON public.usage_events USING btree (event_type);


--
-- Name: idx_user_roles_role_id; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_user_roles_role_id ON public.user_roles USING btree (role_id);


--
-- Name: idx_user_roles_user_id; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_user_roles_user_id ON public.user_roles USING btree (user_id);


--
-- Name: idx_users_is_active; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_users_is_active ON public.users USING btree (is_active);


--
-- Name: idx_wiki_chunks_embedding; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_wiki_chunks_embedding ON public.wiki_chunks USING ivfflat (embedding public.vector_cosine_ops) WITH (lists='100');


--
-- Name: idx_wiki_chunks_entry_id; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_wiki_chunks_entry_id ON public.wiki_chunks USING btree (wiki_entry_id);


--
-- Name: idx_wiki_entries_source_type; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_wiki_entries_source_type ON public.wiki_entries USING btree (source_type);


--
-- Name: idx_workspace_members_active; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_workspace_members_active ON public.workspace_members USING btree (workspace_id, is_active) WHERE (is_active = true);


--
-- Name: idx_workspace_members_user_active; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_workspace_members_user_active ON public.workspace_members USING btree (user_id) WHERE (is_active = true);


--
-- Name: idx_workspace_members_workspace; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_workspace_members_workspace ON public.workspace_members USING btree (workspace_id);


--
-- Name: idx_workspace_themes_workspace_id; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_workspace_themes_workspace_id ON public.workspace_themes USING btree (workspace_id);


--
-- Name: idx_workspaces_active; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_workspaces_active ON public.workspaces USING btree (is_active) WHERE (is_active = true);


--
-- Name: idx_workspaces_org_id; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_workspaces_org_id ON public.workspaces USING btree (organization_id);


--
-- Name: idx_workspaces_slug; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_workspaces_slug ON public.workspaces USING btree (slug);


--
-- Name: idx_ws_ai_settings_ws_id; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_ws_ai_settings_ws_id ON public.workspace_ai_settings USING btree (workspace_id);


--
-- Name: invitations_pending_unique; Type: INDEX; Schema: public; Owner: -
--

CREATE UNIQUE INDEX invitations_pending_unique ON public.invitations USING btree (email, organization_id) WHERE (accepted_at IS NULL);


--
-- Name: reminder_turns_queue; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX reminder_turns_queue ON public.reminder_turns USING btree (created_at, id);


--
-- Name: reminder_turns_reminder; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX reminder_turns_reminder ON public.reminder_turns USING btree (reminder_id);


--
-- Name: reminders_due; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX reminders_due ON public.reminders USING btree (due_at) WHERE (status = 'pending'::text);


--
-- Name: task_runs_active_waiting; Type: INDEX; Schema: public; Owner: -
--

CREATE UNIQUE INDEX task_runs_active_waiting ON public.task_runs USING btree (task_id) WHERE (status = ANY (ARRAY['running'::text, 'waiting'::text]));


--
-- Name: task_runs_heartbeat_waiting; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX task_runs_heartbeat_waiting ON public.task_runs USING btree (heartbeat_at) WHERE (status = ANY (ARRAY['running'::text, 'waiting'::text]));


--
-- Name: content_chunks chunk_search_vector_trigger; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER chunk_search_vector_trigger BEFORE INSERT OR UPDATE OF text ON public.content_chunks FOR EACH ROW EXECUTE FUNCTION public.update_chunk_search_vector();


--
-- Name: content_items item_search_vector_trigger; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER item_search_vector_trigger BEFORE INSERT OR UPDATE OF title, content ON public.content_items FOR EACH ROW EXECUTE FUNCTION public.update_item_search_vector();


--
-- Name: knowledge_entries knowledge_search_vector_trigger; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER knowledge_search_vector_trigger BEFORE INSERT OR UPDATE OF title, content ON public.knowledge_entries FOR EACH ROW EXECUTE FUNCTION public.update_knowledge_search_vector();


--
-- Name: messages message_search_vector_trigger; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER message_search_vector_trigger BEFORE INSERT OR UPDATE OF content ON public.messages FOR EACH ROW EXECUTE FUNCTION public.update_message_search_vector();


--
-- Name: sync_configs sync_configs_updated_at; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER sync_configs_updated_at BEFORE UPDATE ON public.sync_configs FOR EACH ROW EXECUTE FUNCTION public.update_sync_configs_updated_at();


--
-- Name: agent_logins agent_logins_organization_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.agent_logins
    ADD CONSTRAINT agent_logins_organization_id_fkey FOREIGN KEY (organization_id) REFERENCES public.organizations(id) ON DELETE CASCADE;


--
-- Name: audit_logs audit_logs_actor_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.audit_logs
    ADD CONSTRAINT audit_logs_actor_id_fkey FOREIGN KEY (actor_id) REFERENCES public.users(id) ON DELETE SET NULL;


--
-- Name: audit_logs audit_logs_organization_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.audit_logs
    ADD CONSTRAINT audit_logs_organization_id_fkey FOREIGN KEY (organization_id) REFERENCES public.organizations(id) ON DELETE SET NULL;


--
-- Name: audit_logs audit_logs_workspace_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.audit_logs
    ADD CONSTRAINT audit_logs_workspace_id_fkey FOREIGN KEY (workspace_id) REFERENCES public.workspaces(id) ON DELETE SET NULL;


--
-- Name: chat_attached_sources chat_attached_sources_chat_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.chat_attached_sources
    ADD CONSTRAINT chat_attached_sources_chat_id_fkey FOREIGN KEY (chat_id) REFERENCES public.chats(id) ON DELETE CASCADE;


--
-- Name: chat_attached_sources chat_attached_sources_source_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.chat_attached_sources
    ADD CONSTRAINT chat_attached_sources_source_id_fkey FOREIGN KEY (source_id) REFERENCES public.sources(id) ON DELETE CASCADE;


--
-- Name: chat_calls chat_calls_chat_id_envelope_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.chat_calls
    ADD CONSTRAINT chat_calls_chat_id_envelope_id_fkey FOREIGN KEY (chat_id, envelope_id) REFERENCES public.chat_entries(chat_id, id) ON DELETE CASCADE;


--
-- Name: chat_calls chat_calls_chat_id_result_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.chat_calls
    ADD CONSTRAINT chat_calls_chat_id_result_id_fkey FOREIGN KEY (chat_id, result_id) REFERENCES public.chat_entries(chat_id, id) ON DELETE CASCADE;


--
-- Name: chat_calls chat_calls_chat_id_turn_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.chat_calls
    ADD CONSTRAINT chat_calls_chat_id_turn_id_fkey FOREIGN KEY (chat_id, turn_id) REFERENCES public.chat_turns(chat_id, id) ON DELETE CASCADE;


--
-- Name: chat_checkpoints chat_checkpoints_chat_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.chat_checkpoints
    ADD CONSTRAINT chat_checkpoints_chat_id_fkey FOREIGN KEY (chat_id) REFERENCES public.chats(id) ON DELETE CASCADE;


--
-- Name: chat_entries chat_entries_chat_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.chat_entries
    ADD CONSTRAINT chat_entries_chat_id_fkey FOREIGN KEY (chat_id) REFERENCES public.chats(id) ON DELETE CASCADE;


--
-- Name: chat_entries chat_entries_chat_id_turn_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.chat_entries
    ADD CONSTRAINT chat_entries_chat_id_turn_id_fkey FOREIGN KEY (chat_id, turn_id) REFERENCES public.chat_turns(chat_id, id) ON DELETE CASCADE;


--
-- Name: chat_leases chat_leases_chat_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.chat_leases
    ADD CONSTRAINT chat_leases_chat_id_fkey FOREIGN KEY (chat_id) REFERENCES public.chats(id) ON DELETE CASCADE;


--
-- Name: chat_sources chat_sources_chat_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.chat_sources
    ADD CONSTRAINT chat_sources_chat_id_fkey FOREIGN KEY (chat_id) REFERENCES public.chats(id) ON DELETE CASCADE;


--
-- Name: chat_turns chat_turns_chat_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.chat_turns
    ADD CONSTRAINT chat_turns_chat_id_fkey FOREIGN KEY (chat_id) REFERENCES public.chats(id) ON DELETE CASCADE;


--
-- Name: chat_turns chat_turns_user_message_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.chat_turns
    ADD CONSTRAINT chat_turns_user_message_id_fkey FOREIGN KEY (user_message_id) REFERENCES public.messages(id) ON DELETE CASCADE;


--
-- Name: chats chats_project_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.chats
    ADD CONSTRAINT chats_project_id_fkey FOREIGN KEY (project_id) REFERENCES public.projects(id) ON DELETE SET NULL;


--
-- Name: chats chats_workspace_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.chats
    ADD CONSTRAINT chats_workspace_id_fkey FOREIGN KEY (workspace_id) REFERENCES public.workspaces(id) ON DELETE CASCADE;


--
-- Name: content_chunks content_chunks_content_item_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.content_chunks
    ADD CONSTRAINT content_chunks_content_item_id_fkey FOREIGN KEY (content_item_id) REFERENCES public.content_items(id) ON DELETE CASCADE;


--
-- Name: content_items content_items_source_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.content_items
    ADD CONSTRAINT content_items_source_id_fkey FOREIGN KEY (source_id) REFERENCES public.sources(id) ON DELETE CASCADE;


--
-- Name: content_items content_items_workspace_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.content_items
    ADD CONSTRAINT content_items_workspace_id_fkey FOREIGN KEY (workspace_id) REFERENCES public.workspaces(id) ON DELETE CASCADE;


--
-- Name: context_gatherings context_gatherings_task_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.context_gatherings
    ADD CONSTRAINT context_gatherings_task_id_fkey FOREIGN KEY (task_id) REFERENCES public.tasks(id) ON DELETE SET NULL;


--
-- Name: context_gatherings context_gatherings_user_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.context_gatherings
    ADD CONSTRAINT context_gatherings_user_id_fkey FOREIGN KEY (user_id) REFERENCES public.users(id) ON DELETE SET NULL;


--
-- Name: context_gatherings context_gatherings_workspace_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.context_gatherings
    ADD CONSTRAINT context_gatherings_workspace_id_fkey FOREIGN KEY (workspace_id) REFERENCES public.workspaces(id) ON DELETE SET NULL;


--
-- Name: email_verification_tokens email_verification_tokens_user_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.email_verification_tokens
    ADD CONSTRAINT email_verification_tokens_user_id_fkey FOREIGN KEY (user_id) REFERENCES public.users(id) ON DELETE CASCADE;


--
-- Name: embeddings embeddings_chunk_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.embeddings
    ADD CONSTRAINT embeddings_chunk_id_fkey FOREIGN KEY (chunk_id) REFERENCES public.content_chunks(id) ON DELETE CASCADE;


--
-- Name: embeddings embeddings_content_item_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.embeddings
    ADD CONSTRAINT embeddings_content_item_id_fkey FOREIGN KEY (content_item_id) REFERENCES public.content_items(id) ON DELETE CASCADE;


--
-- Name: embeddings embeddings_source_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.embeddings
    ADD CONSTRAINT embeddings_source_id_fkey FOREIGN KEY (source_id) REFERENCES public.sources(id) ON DELETE CASCADE;


--
-- Name: embeddings embeddings_workspace_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.embeddings
    ADD CONSTRAINT embeddings_workspace_id_fkey FOREIGN KEY (workspace_id) REFERENCES public.workspaces(id) ON DELETE CASCADE;


--
-- Name: gathering_events gathering_events_gathering_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.gathering_events
    ADD CONSTRAINT gathering_events_gathering_id_fkey FOREIGN KEY (gathering_id) REFERENCES public.context_gatherings(id) ON DELETE CASCADE;


--
-- Name: heuristic_analysis heuristic_analysis_content_item_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.heuristic_analysis
    ADD CONSTRAINT heuristic_analysis_content_item_id_fkey FOREIGN KEY (content_item_id) REFERENCES public.content_items(id) ON DELETE CASCADE;


--
-- Name: invitations invitations_invited_by_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.invitations
    ADD CONSTRAINT invitations_invited_by_fkey FOREIGN KEY (invited_by) REFERENCES public.users(id);


--
-- Name: invitations invitations_organization_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.invitations
    ADD CONSTRAINT invitations_organization_id_fkey FOREIGN KEY (organization_id) REFERENCES public.organizations(id) ON DELETE CASCADE;


--
-- Name: knowledge_embeddings knowledge_embeddings_knowledge_entry_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.knowledge_embeddings
    ADD CONSTRAINT knowledge_embeddings_knowledge_entry_id_fkey FOREIGN KEY (knowledge_entry_id) REFERENCES public.knowledge_entries(id) ON DELETE CASCADE;


--
-- Name: knowledge_embeddings knowledge_embeddings_workspace_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.knowledge_embeddings
    ADD CONSTRAINT knowledge_embeddings_workspace_id_fkey FOREIGN KEY (workspace_id) REFERENCES public.workspaces(id) ON DELETE CASCADE;


--
-- Name: knowledge_entries knowledge_entries_created_by_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.knowledge_entries
    ADD CONSTRAINT knowledge_entries_created_by_fkey FOREIGN KEY (created_by) REFERENCES public.users(id) ON DELETE SET NULL;


--
-- Name: knowledge_entries knowledge_entries_workspace_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.knowledge_entries
    ADD CONSTRAINT knowledge_entries_workspace_id_fkey FOREIGN KEY (workspace_id) REFERENCES public.workspaces(id) ON DELETE CASCADE;


--
-- Name: message_embeddings message_embeddings_chat_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.message_embeddings
    ADD CONSTRAINT message_embeddings_chat_id_fkey FOREIGN KEY (chat_id) REFERENCES public.chats(id) ON DELETE CASCADE;


--
-- Name: message_embeddings message_embeddings_message_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.message_embeddings
    ADD CONSTRAINT message_embeddings_message_id_fkey FOREIGN KEY (message_id) REFERENCES public.messages(id) ON DELETE CASCADE;


--
-- Name: message_embeddings message_embeddings_workspace_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.message_embeddings
    ADD CONSTRAINT message_embeddings_workspace_id_fkey FOREIGN KEY (workspace_id) REFERENCES public.workspaces(id) ON DELETE CASCADE;


--
-- Name: messages messages_chat_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.messages
    ADD CONSTRAINT messages_chat_id_fkey FOREIGN KEY (chat_id) REFERENCES public.chats(id) ON DELETE CASCADE;


--
-- Name: organization_ai_settings organization_ai_settings_organization_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.organization_ai_settings
    ADD CONSTRAINT organization_ai_settings_organization_id_fkey FOREIGN KEY (organization_id) REFERENCES public.organizations(id) ON DELETE CASCADE;


--
-- Name: organization_members organization_members_invited_by_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.organization_members
    ADD CONSTRAINT organization_members_invited_by_fkey FOREIGN KEY (invited_by) REFERENCES public.users(id) ON DELETE SET NULL;


--
-- Name: organization_members organization_members_organization_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.organization_members
    ADD CONSTRAINT organization_members_organization_id_fkey FOREIGN KEY (organization_id) REFERENCES public.organizations(id) ON DELETE CASCADE;


--
-- Name: organization_members organization_members_user_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.organization_members
    ADD CONSTRAINT organization_members_user_id_fkey FOREIGN KEY (user_id) REFERENCES public.users(id) ON DELETE CASCADE;


--
-- Name: password_reset_tokens password_reset_tokens_user_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.password_reset_tokens
    ADD CONSTRAINT password_reset_tokens_user_id_fkey FOREIGN KEY (user_id) REFERENCES public.users(id) ON DELETE CASCADE;


--
-- Name: projects projects_auto_actor_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.projects
    ADD CONSTRAINT projects_auto_actor_id_fkey FOREIGN KEY (auto_actor_id) REFERENCES public.users(id) ON DELETE SET NULL;


--
-- Name: projects projects_source_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.projects
    ADD CONSTRAINT projects_source_id_fkey FOREIGN KEY (source_id) REFERENCES public.sources(id) ON DELETE SET NULL;


--
-- Name: projects projects_workspace_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.projects
    ADD CONSTRAINT projects_workspace_id_fkey FOREIGN KEY (workspace_id) REFERENCES public.workspaces(id) ON DELETE CASCADE;


--
-- Name: refresh_tokens refresh_tokens_user_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.refresh_tokens
    ADD CONSTRAINT refresh_tokens_user_id_fkey FOREIGN KEY (user_id) REFERENCES public.users(id) ON DELETE CASCADE;


--
-- Name: reminder_turns reminder_turns_chat_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.reminder_turns
    ADD CONSTRAINT reminder_turns_chat_id_fkey FOREIGN KEY (chat_id) REFERENCES public.chats(id) ON DELETE CASCADE;


--
-- Name: reminder_turns reminder_turns_created_by_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.reminder_turns
    ADD CONSTRAINT reminder_turns_created_by_fkey FOREIGN KEY (created_by) REFERENCES public.users(id) ON DELETE CASCADE;


--
-- Name: reminder_turns reminder_turns_reminder_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.reminder_turns
    ADD CONSTRAINT reminder_turns_reminder_id_fkey FOREIGN KEY (reminder_id) REFERENCES public.reminders(id) ON DELETE CASCADE;


--
-- Name: reminder_turns reminder_turns_workspace_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.reminder_turns
    ADD CONSTRAINT reminder_turns_workspace_id_fkey FOREIGN KEY (workspace_id) REFERENCES public.workspaces(id) ON DELETE CASCADE;


--
-- Name: reminders reminders_chat_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.reminders
    ADD CONSTRAINT reminders_chat_id_fkey FOREIGN KEY (chat_id) REFERENCES public.chats(id) ON DELETE CASCADE;


--
-- Name: reminders reminders_created_by_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.reminders
    ADD CONSTRAINT reminders_created_by_fkey FOREIGN KEY (created_by) REFERENCES public.users(id) ON DELETE CASCADE;


--
-- Name: reminders reminders_message_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.reminders
    ADD CONSTRAINT reminders_message_id_fkey FOREIGN KEY (message_id) REFERENCES public.messages(id) ON DELETE SET NULL;


--
-- Name: reminders reminders_workspace_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.reminders
    ADD CONSTRAINT reminders_workspace_id_fkey FOREIGN KEY (workspace_id) REFERENCES public.workspaces(id) ON DELETE CASCADE;


--
-- Name: role_permissions role_permissions_permission_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.role_permissions
    ADD CONSTRAINT role_permissions_permission_id_fkey FOREIGN KEY (permission_id) REFERENCES public.permissions(id) ON DELETE CASCADE;


--
-- Name: role_permissions role_permissions_role_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.role_permissions
    ADD CONSTRAINT role_permissions_role_id_fkey FOREIGN KEY (role_id) REFERENCES public.roles(id) ON DELETE CASCADE;


--
-- Name: sessions sessions_user_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.sessions
    ADD CONSTRAINT sessions_user_id_fkey FOREIGN KEY (user_id) REFERENCES public.users(id) ON DELETE CASCADE;


--
-- Name: source_sync_state source_sync_state_source_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.source_sync_state
    ADD CONSTRAINT source_sync_state_source_id_fkey FOREIGN KEY (source_id) REFERENCES public.sources(id) ON DELETE CASCADE;


--
-- Name: sources sources_source_type_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.sources
    ADD CONSTRAINT sources_source_type_fkey FOREIGN KEY (source_type) REFERENCES public.source_types(name);


--
-- Name: sources sources_workspace_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.sources
    ADD CONSTRAINT sources_workspace_id_fkey FOREIGN KEY (workspace_id) REFERENCES public.workspaces(id) ON DELETE CASCADE;


--
-- Name: subscriptions subscriptions_organization_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.subscriptions
    ADD CONSTRAINT subscriptions_organization_id_fkey FOREIGN KEY (organization_id) REFERENCES public.organizations(id) ON DELETE CASCADE;


--
-- Name: subscriptions subscriptions_plan_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.subscriptions
    ADD CONSTRAINT subscriptions_plan_id_fkey FOREIGN KEY (plan_id) REFERENCES public.plans(id);


--
-- Name: sync_configs sync_configs_project_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.sync_configs
    ADD CONSTRAINT sync_configs_project_id_fkey FOREIGN KEY (project_id) REFERENCES public.projects(id) ON DELETE CASCADE;


--
-- Name: sync_deliveries sync_deliveries_sync_config_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.sync_deliveries
    ADD CONSTRAINT sync_deliveries_sync_config_id_fkey FOREIGN KEY (sync_config_id) REFERENCES public.sync_configs(id) ON DELETE CASCADE;


--
-- Name: sync_events sync_events_sync_config_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.sync_events
    ADD CONSTRAINT sync_events_sync_config_id_fkey FOREIGN KEY (sync_config_id) REFERENCES public.sync_configs(id) ON DELETE CASCADE;


--
-- Name: sync_events sync_events_synced_item_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.sync_events
    ADD CONSTRAINT sync_events_synced_item_id_fkey FOREIGN KEY (synced_item_id) REFERENCES public.synced_items(id) ON DELETE SET NULL;


--
-- Name: sync_unlinked_items sync_unlinked_items_sync_config_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.sync_unlinked_items
    ADD CONSTRAINT sync_unlinked_items_sync_config_id_fkey FOREIGN KEY (sync_config_id) REFERENCES public.sync_configs(id) ON DELETE CASCADE;


--
-- Name: synced_items synced_items_sync_config_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.synced_items
    ADD CONSTRAINT synced_items_sync_config_id_fkey FOREIGN KEY (sync_config_id) REFERENCES public.sync_configs(id) ON DELETE CASCADE;


--
-- Name: synced_items synced_items_task_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.synced_items
    ADD CONSTRAINT synced_items_task_id_fkey FOREIGN KEY (task_id) REFERENCES public.tasks(id) ON DELETE CASCADE;


--
-- Name: task_automation task_automation_last_run_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.task_automation
    ADD CONSTRAINT task_automation_last_run_id_fkey FOREIGN KEY (last_run_id) REFERENCES public.task_runs(id) ON DELETE SET NULL;


--
-- Name: task_automation task_automation_project_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.task_automation
    ADD CONSTRAINT task_automation_project_id_fkey FOREIGN KEY (project_id) REFERENCES public.projects(id) ON DELETE CASCADE;


--
-- Name: task_automation task_automation_task_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.task_automation
    ADD CONSTRAINT task_automation_task_id_fkey FOREIGN KEY (task_id) REFERENCES public.tasks(id) ON DELETE CASCADE;


--
-- Name: task_file_changes task_file_changes_task_run_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.task_file_changes
    ADD CONSTRAINT task_file_changes_task_run_id_fkey FOREIGN KEY (task_run_id) REFERENCES public.task_runs(id) ON DELETE CASCADE;


--
-- Name: task_projects task_projects_project_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.task_projects
    ADD CONSTRAINT task_projects_project_id_fkey FOREIGN KEY (project_id) REFERENCES public.projects(id) ON DELETE CASCADE;


--
-- Name: task_projects task_projects_task_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.task_projects
    ADD CONSTRAINT task_projects_task_id_fkey FOREIGN KEY (task_id) REFERENCES public.tasks(id) ON DELETE CASCADE;


--
-- Name: task_queue task_queue_task_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.task_queue
    ADD CONSTRAINT task_queue_task_id_fkey FOREIGN KEY (task_id) REFERENCES public.tasks(id) ON DELETE CASCADE;


--
-- Name: task_reviews task_reviews_run_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.task_reviews
    ADD CONSTRAINT task_reviews_run_id_fkey FOREIGN KEY (run_id) REFERENCES public.task_runs(id) ON DELETE SET NULL;


--
-- Name: task_reviews task_reviews_task_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.task_reviews
    ADD CONSTRAINT task_reviews_task_id_fkey FOREIGN KEY (task_id) REFERENCES public.tasks(id) ON DELETE CASCADE;


--
-- Name: task_run_checkouts task_run_checkouts_run_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.task_run_checkouts
    ADD CONSTRAINT task_run_checkouts_run_id_fkey FOREIGN KEY (run_id) REFERENCES public.task_runs(id) ON DELETE CASCADE;


--
-- Name: task_run_logs task_run_logs_task_run_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.task_run_logs
    ADD CONSTRAINT task_run_logs_task_run_id_fkey FOREIGN KEY (task_run_id) REFERENCES public.task_runs(id) ON DELETE CASCADE;


--
-- Name: task_runs task_runs_task_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.task_runs
    ADD CONSTRAINT task_runs_task_id_fkey FOREIGN KEY (task_id) REFERENCES public.tasks(id) ON DELETE CASCADE;


--
-- Name: task_runs task_runs_triggered_by_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.task_runs
    ADD CONSTRAINT task_runs_triggered_by_fkey FOREIGN KEY (triggered_by) REFERENCES public.users(id) ON DELETE SET NULL;


--
-- Name: task_tool_calls task_tool_calls_task_run_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.task_tool_calls
    ADD CONSTRAINT task_tool_calls_task_run_id_fkey FOREIGN KEY (task_run_id) REFERENCES public.task_runs(id) ON DELETE CASCADE;


--
-- Name: tasks tasks_active_run_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.tasks
    ADD CONSTRAINT tasks_active_run_id_fkey FOREIGN KEY (active_run_id) REFERENCES public.task_runs(id) ON DELETE SET NULL;


--
-- Name: tasks tasks_assignee_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.tasks
    ADD CONSTRAINT tasks_assignee_id_fkey FOREIGN KEY (assignee_id) REFERENCES public.users(id) ON DELETE SET NULL;


--
-- Name: tasks tasks_created_by_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.tasks
    ADD CONSTRAINT tasks_created_by_fkey FOREIGN KEY (created_by) REFERENCES public.users(id) ON DELETE SET NULL;


--
-- Name: tasks tasks_source_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.tasks
    ADD CONSTRAINT tasks_source_id_fkey FOREIGN KEY (source_id) REFERENCES public.sources(id) ON DELETE SET NULL;


--
-- Name: tasks tasks_workspace_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.tasks
    ADD CONSTRAINT tasks_workspace_id_fkey FOREIGN KEY (workspace_id) REFERENCES public.workspaces(id) ON DELETE CASCADE;


--
-- Name: usage_events usage_events_organization_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.usage_events
    ADD CONSTRAINT usage_events_organization_id_fkey FOREIGN KEY (organization_id) REFERENCES public.organizations(id) ON DELETE CASCADE;


--
-- Name: usage_events usage_events_user_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.usage_events
    ADD CONSTRAINT usage_events_user_id_fkey FOREIGN KEY (user_id) REFERENCES public.users(id) ON DELETE SET NULL;


--
-- Name: usage_events usage_events_workspace_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.usage_events
    ADD CONSTRAINT usage_events_workspace_id_fkey FOREIGN KEY (workspace_id) REFERENCES public.workspaces(id) ON DELETE SET NULL;


--
-- Name: user_roles user_roles_assigned_by_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.user_roles
    ADD CONSTRAINT user_roles_assigned_by_fkey FOREIGN KEY (assigned_by) REFERENCES public.users(id);


--
-- Name: user_roles user_roles_role_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.user_roles
    ADD CONSTRAINT user_roles_role_id_fkey FOREIGN KEY (role_id) REFERENCES public.roles(id) ON DELETE CASCADE;


--
-- Name: user_roles user_roles_user_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.user_roles
    ADD CONSTRAINT user_roles_user_id_fkey FOREIGN KEY (user_id) REFERENCES public.users(id) ON DELETE CASCADE;


--
-- Name: wiki_chunks wiki_chunks_wiki_entry_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.wiki_chunks
    ADD CONSTRAINT wiki_chunks_wiki_entry_id_fkey FOREIGN KEY (wiki_entry_id) REFERENCES public.wiki_entries(id) ON DELETE CASCADE;


--
-- Name: workspace_ai_settings workspace_ai_settings_workspace_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workspace_ai_settings
    ADD CONSTRAINT workspace_ai_settings_workspace_id_fkey FOREIGN KEY (workspace_id) REFERENCES public.workspaces(id) ON DELETE CASCADE;


--
-- Name: workspace_members workspace_members_invited_by_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workspace_members
    ADD CONSTRAINT workspace_members_invited_by_fkey FOREIGN KEY (invited_by) REFERENCES public.users(id) ON DELETE SET NULL;


--
-- Name: workspace_members workspace_members_user_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workspace_members
    ADD CONSTRAINT workspace_members_user_id_fkey FOREIGN KEY (user_id) REFERENCES public.users(id) ON DELETE CASCADE;


--
-- Name: workspace_members workspace_members_workspace_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workspace_members
    ADD CONSTRAINT workspace_members_workspace_id_fkey FOREIGN KEY (workspace_id) REFERENCES public.workspaces(id) ON DELETE CASCADE;


--
-- Name: workspace_themes workspace_themes_workspace_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workspace_themes
    ADD CONSTRAINT workspace_themes_workspace_id_fkey FOREIGN KEY (workspace_id) REFERENCES public.workspaces(id) ON DELETE CASCADE;


--
-- Name: workspaces workspaces_organization_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workspaces
    ADD CONSTRAINT workspaces_organization_id_fkey FOREIGN KEY (organization_id) REFERENCES public.organizations(id) ON DELETE CASCADE;


--
--

-- Seed data. Idempotent so a replay after a crash window is a no-op.

INSERT INTO source_types (name, category, description, config_schema) VALUES
  ('github', 'file', 'GitHub repository', '{"type":"object","required":["owner","repo"],"properties":{"owner":{"type":"string"},"repo":{"type":"string"},"branch":{"type":"string","default":"main"},"base_path":{"type":"string","default":""}}}'),
  ('gitlab', 'file', 'GitLab repository', '{"type":"object","required":["project_id"],"properties":{"project_id":{"type":"string"},"host":{"type":"string","default":"https://gitlab.com"},"branch":{"type":"string","default":"main"},"base_path":{"type":"string","default":""}}}'),
  ('filesystem', 'file', 'Local filesystem (self-hosted only)', '{"type":"object","required":["base_path"],"properties":{"base_path":{"type":"string"},"allow_writes":{"type":"boolean","default":true}}}'),
  ('ical', 'calendar', 'iCalendar subscription URL', '{"type":"object","required":["url"],"properties":{"url":{"type":"string"},"refresh_interval":{"type":"integer","default":3600}}}'),
  ('imap', 'mail', 'IMAP mail server', '{"type":"object","required":["host","port","username"],"properties":{"host":{"type":"string"},"port":{"type":"integer","default":993},"username":{"type":"string"},"use_ssl":{"type":"boolean","default":true},"folder":{"type":"string","default":"INBOX"}}}'),
  ('discord', 'chat', 'Discord server', '{"type":"object","required":["server_id"],"properties":{"server_id":{"type":"string"},"channel_ids":{"type":"array","items":{"type":"string"}}}}'),
  ('slack', 'chat', 'Slack workspace', '{"type":"object","required":["workspace_id"],"properties":{"workspace_id":{"type":"string"},"channel_ids":{"type":"array","items":{"type":"string"}}}}'),
  ('web', 'web', 'Web URL content fetcher', '{"type":"object","required":["url"],"properties":{"url":{"type":"string"},"headers":{"type":"object"}}}'),
  ('text', 'text', 'Raw text/string content', '{"type":"object","required":["content"],"properties":{"content":{"type":"string"},"label":{"type":"string"}}}')
ON CONFLICT (name) DO NOTHING;

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
  ('workspaces:delete', 'Delete workspaces', 'workspaces', 'delete')
ON CONFLICT (name) DO NOTHING;

INSERT INTO roles (id, name, description, is_system) VALUES
  ('00000000-0000-0000-0000-000000000001', 'admin', 'Full system access', TRUE),
  ('00000000-0000-0000-0000-000000000002', 'user', 'Standard user access', TRUE),
  ('00000000-0000-0000-0000-000000000003', 'viewer', 'Read-only access', TRUE)
ON CONFLICT (id) DO NOTHING;

INSERT INTO role_permissions (role_id, permission_id)
SELECT '00000000-0000-0000-0000-000000000001', id FROM permissions
ON CONFLICT DO NOTHING;

INSERT INTO role_permissions (role_id, permission_id)
SELECT '00000000-0000-0000-0000-000000000002', id FROM permissions
WHERE name IN (
  'projects:create', 'projects:read', 'projects:update', 'projects:delete',
  'tasks:create', 'tasks:read', 'tasks:update', 'tasks:delete',
  'chats:create', 'chats:read', 'chats:update', 'chats:delete',
  'sources:read', 'models:read', 'wiki:read', 'wiki:create', 'wiki:update',
  'organizations:create', 'organizations:read', 'organizations:update',
  'workspaces:create', 'workspaces:read', 'workspaces:update'
)
ON CONFLICT DO NOTHING;

INSERT INTO role_permissions (role_id, permission_id)
SELECT '00000000-0000-0000-0000-000000000003', id FROM permissions WHERE action = 'read'
ON CONFLICT DO NOTHING;

INSERT INTO organizations (id, name, slug, description, is_active) VALUES
  ('00000000-0000-0000-0000-000000000001', 'Default Organization', 'default', 'Default organization for self-hosted installation', true)
ON CONFLICT (id) DO NOTHING;

INSERT INTO workspaces (id, organization_id, name, slug, description, is_active) VALUES
  ('00000000-0000-0000-0000-000000000001', '00000000-0000-0000-0000-000000000001', 'Default Workspace', 'default', 'Default workspace for self-hosted installation', true)
ON CONFLICT (id) DO NOTHING;

INSERT INTO plans (name, slug, description, price_monthly_cents, price_yearly_cents, features, limits) VALUES
  ('Free', 'free', 'For individuals and small teams', 0, 0, '{"api_access": true}'::jsonb, '{"max_workspaces": 1, "max_members": 3, "max_chats_per_month": 100}'::jsonb),
  ('Pro', 'pro', 'For growing teams', 2900, 29000, '{"api_access": true, "priority_support": true}'::jsonb, '{"max_workspaces": 10, "max_members": 25, "max_chats_per_month": 5000}'::jsonb),
  ('Enterprise', 'enterprise', 'For large organizations', 9900, 99000, '{"api_access": true, "priority_support": true, "sso": true, "audit_log": true}'::jsonb, '{"max_workspaces": -1, "max_members": -1, "max_chats_per_month": -1}'::jsonb)
ON CONFLICT (slug) DO NOTHING;
