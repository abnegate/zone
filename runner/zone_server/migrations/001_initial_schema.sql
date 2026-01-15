-- Zone Manager Initial Schema (Squashed)
-- All migrations combined into single initial schema

BEGIN;

-- =============================================================================
-- Extensions
-- =============================================================================
CREATE EXTENSION IF NOT EXISTS vector;

-- =============================================================================
-- Migration Tracking
-- =============================================================================
CREATE TABLE IF NOT EXISTS schema_migrations (
  version INTEGER PRIMARY KEY,
  applied_at TIMESTAMP DEFAULT NOW()
);

-- =============================================================================
-- Organizations & Workspaces
-- =============================================================================
CREATE TABLE IF NOT EXISTS organizations (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  name TEXT NOT NULL,
  slug TEXT NOT NULL UNIQUE,
  description TEXT,
  is_active BOOLEAN DEFAULT TRUE,
  created_at TIMESTAMP DEFAULT NOW(),
  updated_at TIMESTAMP DEFAULT NOW()
);

-- REMOVED: idx_organizations_slug (UNIQUE constraint creates implicit index)
CREATE INDEX IF NOT EXISTS idx_organizations_active ON organizations(is_active) WHERE is_active = TRUE;

CREATE TABLE IF NOT EXISTS workspaces (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  organization_id UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
  name TEXT NOT NULL,
  slug TEXT NOT NULL,
  description TEXT,
  is_active BOOLEAN DEFAULT TRUE,
  created_at TIMESTAMP DEFAULT NOW(),
  updated_at TIMESTAMP DEFAULT NOW(),
  UNIQUE(organization_id, slug)
);

CREATE INDEX IF NOT EXISTS idx_workspaces_org_id ON workspaces(organization_id);
CREATE INDEX IF NOT EXISTS idx_workspaces_slug ON workspaces(slug);
CREATE INDEX IF NOT EXISTS idx_workspaces_active ON workspaces(is_active) WHERE is_active = TRUE;

-- =============================================================================
-- Users & Authentication
-- =============================================================================
CREATE TABLE IF NOT EXISTS users (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  email TEXT NOT NULL UNIQUE,
  password_hash TEXT NOT NULL,
  display_name TEXT,
  is_active BOOLEAN DEFAULT TRUE,
  is_admin BOOLEAN DEFAULT FALSE,
  email_verified BOOLEAN NOT NULL DEFAULT FALSE,
  created_at TIMESTAMP DEFAULT NOW(),
  updated_at TIMESTAMP DEFAULT NOW(),
  last_login_at TIMESTAMP
);

-- REMOVED: idx_users_email (UNIQUE constraint creates implicit index)
CREATE INDEX IF NOT EXISTS idx_users_is_active ON users(is_active);

CREATE TABLE IF NOT EXISTS refresh_tokens (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  token_hash TEXT NOT NULL UNIQUE,
  expires_at TIMESTAMP NOT NULL,
  created_at TIMESTAMP DEFAULT NOW(),
  revoked_at TIMESTAMP,
  user_agent TEXT,
  ip_address TEXT
);

CREATE INDEX IF NOT EXISTS idx_refresh_tokens_user_id ON refresh_tokens(user_id);
-- REMOVED: idx_refresh_tokens_token_hash (UNIQUE constraint creates implicit index)
CREATE INDEX IF NOT EXISTS idx_refresh_tokens_expires_at ON refresh_tokens(expires_at);

CREATE TABLE IF NOT EXISTS sessions (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  refresh_token_hash VARCHAR(255) NOT NULL UNIQUE,
  ip_address INET,
  user_agent TEXT,
  device_info JSONB,
  last_active_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  expires_at TIMESTAMPTZ NOT NULL,
  revoked_at TIMESTAMPTZ,
  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- REMOVED: idx_sessions_token (UNIQUE constraint creates implicit index)
-- ADDED: Composite partial index for active session queries (WHERE revoked_at IS NULL)
CREATE INDEX IF NOT EXISTS idx_sessions_user_active ON sessions(user_id, last_active_at DESC) WHERE revoked_at IS NULL;
CREATE INDEX IF NOT EXISTS idx_sessions_expires ON sessions(expires_at);

CREATE TABLE IF NOT EXISTS email_verification_tokens (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  token_hash VARCHAR(255) NOT NULL UNIQUE,
  expires_at TIMESTAMPTZ NOT NULL,
  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_email_verification_tokens_user ON email_verification_tokens(user_id);
-- REMOVED: idx_email_verification_tokens_token_hash (UNIQUE constraint creates implicit index)

CREATE TABLE IF NOT EXISTS password_reset_tokens (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  token_hash VARCHAR(255) NOT NULL UNIQUE,
  expires_at TIMESTAMPTZ NOT NULL,
  used_at TIMESTAMPTZ,
  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_password_reset_tokens_user ON password_reset_tokens(user_id);
-- REMOVED: idx_password_reset_tokens_token_hash (UNIQUE constraint creates implicit index)

-- =============================================================================
-- Permissions & Roles (RBAC)
-- =============================================================================
CREATE TABLE IF NOT EXISTS permissions (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  name TEXT NOT NULL UNIQUE,
  description TEXT,
  resource TEXT NOT NULL,
  action TEXT NOT NULL CHECK (action IN ('create', 'read', 'update', 'delete')),
  created_at TIMESTAMP DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_permissions_resource ON permissions(resource);

CREATE TABLE IF NOT EXISTS roles (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  name TEXT NOT NULL UNIQUE,
  description TEXT,
  is_system BOOLEAN DEFAULT FALSE,
  created_at TIMESTAMP DEFAULT NOW(),
  updated_at TIMESTAMP DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS role_permissions (
  role_id UUID NOT NULL REFERENCES roles(id) ON DELETE CASCADE,
  permission_id UUID NOT NULL REFERENCES permissions(id) ON DELETE CASCADE,
  PRIMARY KEY (role_id, permission_id)
);

CREATE INDEX IF NOT EXISTS idx_role_permissions_role_id ON role_permissions(role_id);

CREATE TABLE IF NOT EXISTS user_roles (
  user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  role_id UUID NOT NULL REFERENCES roles(id) ON DELETE CASCADE,
  assigned_at TIMESTAMP DEFAULT NOW(),
  assigned_by UUID REFERENCES users(id),
  PRIMARY KEY (user_id, role_id)
);

CREATE INDEX IF NOT EXISTS idx_user_roles_user_id ON user_roles(user_id);
CREATE INDEX IF NOT EXISTS idx_user_roles_role_id ON user_roles(role_id);

-- =============================================================================
-- Organization & Workspace Membership
-- =============================================================================
CREATE TABLE IF NOT EXISTS organization_members (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  organization_id UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
  user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  role TEXT NOT NULL DEFAULT 'member' CHECK (role IN ('owner', 'admin', 'member')),
  is_active BOOLEAN NOT NULL DEFAULT TRUE,
  invited_by UUID REFERENCES users(id) ON DELETE SET NULL,
  invited_at TIMESTAMP,
  accepted_at TIMESTAMP,
  created_at TIMESTAMP DEFAULT NOW(),
  updated_at TIMESTAMP DEFAULT NOW(),
  UNIQUE(organization_id, user_id)
);

CREATE INDEX IF NOT EXISTS idx_org_members_org ON organization_members(organization_id);
-- ADDED: Partial index for active member queries (WHERE is_active = TRUE)
CREATE INDEX IF NOT EXISTS idx_org_members_user_active ON organization_members(user_id) WHERE is_active = TRUE;
CREATE INDEX IF NOT EXISTS idx_org_members_active ON organization_members(organization_id, is_active) WHERE is_active = TRUE;

CREATE TABLE IF NOT EXISTS workspace_members (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  workspace_id UUID NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
  user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  role TEXT NOT NULL DEFAULT 'member' CHECK (role IN ('owner', 'admin', 'member', 'viewer')),
  is_active BOOLEAN NOT NULL DEFAULT TRUE,
  invited_by UUID REFERENCES users(id) ON DELETE SET NULL,
  invited_at TIMESTAMP,
  accepted_at TIMESTAMP,
  created_at TIMESTAMP DEFAULT NOW(),
  updated_at TIMESTAMP DEFAULT NOW(),
  UNIQUE(workspace_id, user_id)
);

CREATE INDEX IF NOT EXISTS idx_workspace_members_workspace ON workspace_members(workspace_id);
-- ADDED: Partial index for active member queries (WHERE is_active = TRUE)
CREATE INDEX IF NOT EXISTS idx_workspace_members_user_active ON workspace_members(user_id) WHERE is_active = TRUE;
CREATE INDEX IF NOT EXISTS idx_workspace_members_active ON workspace_members(workspace_id, is_active) WHERE is_active = TRUE;

CREATE TABLE IF NOT EXISTS invitations (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  email VARCHAR(255) NOT NULL,
  organization_id UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
  workspace_ids UUID[] DEFAULT '{}',
  org_role VARCHAR(50) NOT NULL DEFAULT 'member',
  workspace_role VARCHAR(50) NOT NULL DEFAULT 'member',
  token_hash VARCHAR(255) NOT NULL UNIQUE,
  invited_by UUID NOT NULL REFERENCES users(id),
  expires_at TIMESTAMPTZ NOT NULL,
  accepted_at TIMESTAMPTZ,
  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  UNIQUE(email, organization_id)
);

-- ADDED: Partial composite index for pending invitation queries (WHERE accepted_at IS NULL)
CREATE INDEX IF NOT EXISTS idx_invitations_pending ON invitations(email, organization_id, expires_at) WHERE accepted_at IS NULL;
CREATE INDEX IF NOT EXISTS idx_invitations_token ON invitations(token_hash);
CREATE INDEX IF NOT EXISTS idx_invitations_org ON invitations(organization_id);

-- =============================================================================
-- Sources
-- =============================================================================
CREATE TABLE IF NOT EXISTS source_types (
  name TEXT PRIMARY KEY,
  category TEXT NOT NULL DEFAULT 'file',
  description TEXT NOT NULL,
  config_schema JSONB NOT NULL DEFAULT '{}'::jsonb,
  created_at TIMESTAMP DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_source_types_category ON source_types(category);

CREATE TABLE IF NOT EXISTS sources (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  workspace_id UUID REFERENCES workspaces(id) ON DELETE CASCADE,
  name TEXT NOT NULL,
  source_type TEXT NOT NULL REFERENCES source_types(name),
  config JSONB NOT NULL DEFAULT '{}'::jsonb,
  credentials_encrypted TEXT,
  description TEXT,
  url TEXT,
  is_active BOOLEAN DEFAULT TRUE,
  last_verified_at TIMESTAMP,
  last_error TEXT,
  created_at TIMESTAMP DEFAULT NOW(),
  updated_at TIMESTAMP DEFAULT NOW(),
  UNIQUE(name, source_type)
);

-- ADDED: Composite index for filtered source queries (workspace_id, source_type, is_active, created_at DESC)
CREATE INDEX IF NOT EXISTS idx_sources_workspace_filters ON sources(workspace_id, source_type, is_active, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_sources_active ON sources(is_active) WHERE is_active = TRUE;

CREATE TABLE IF NOT EXISTS source_sync_state (
  source_id UUID PRIMARY KEY REFERENCES sources(id) ON DELETE CASCADE,
  last_sync_at TIMESTAMP,
  cursor TEXT,
  etag TEXT,
  version TEXT,
  extra JSONB NOT NULL DEFAULT '{}'::jsonb,
  created_at TIMESTAMP DEFAULT NOW(),
  updated_at TIMESTAMP DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_sync_state_last_sync ON source_sync_state(last_sync_at);

-- =============================================================================
-- Projects
-- =============================================================================
CREATE TABLE IF NOT EXISTS projects (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  workspace_id UUID REFERENCES workspaces(id) ON DELETE CASCADE,
  source_id UUID REFERENCES sources(id) ON DELETE SET NULL,
  name TEXT NOT NULL,
  description TEXT,
  status TEXT NOT NULL DEFAULT 'active' CHECK (status IN ('active', 'on_hold', 'cancelled')),
  github_repo_url TEXT,
  github_access_token TEXT,
  created_at TIMESTAMP DEFAULT NOW(),
  updated_at TIMESTAMP DEFAULT NOW()
);

-- ADDED: Composite index for filtered project queries (workspace_id, status, created_at DESC)
CREATE INDEX IF NOT EXISTS idx_projects_workspace_status ON projects(workspace_id, status, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_projects_status ON projects(status);

-- =============================================================================
-- Tasks (workspace-scoped, many-to-many with projects)
-- =============================================================================
CREATE TABLE IF NOT EXISTS tasks (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  workspace_id UUID NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
  title TEXT NOT NULL,
  description TEXT NOT NULL,
  acceptance_criteria TEXT,
  status TEXT NOT NULL DEFAULT 'created' CHECK (status IN
    ('created', 'queued', 'in_progress', 'review', 'complete', 'blocked')),
  priority INTEGER CHECK (priority >= 1 AND priority <= 5),
  model_name TEXT,
  dependencies JSONB DEFAULT '[]'::jsonb,
  is_agentic BOOLEAN NOT NULL DEFAULT FALSE,
  github_repo_url TEXT,
  source_id UUID REFERENCES sources(id) ON DELETE SET NULL,
  source_ids UUID[] DEFAULT '{}',
  worker_id TEXT,
  queued_at TIMESTAMP,
  started_at TIMESTAMP,
  completed_at TIMESTAMP,
  created_at TIMESTAMP DEFAULT NOW(),
  updated_at TIMESTAMP DEFAULT NOW(),
  pr_url TEXT,
  branch_name TEXT,
  pr_status TEXT,
  pr_created_at TIMESTAMP
);

-- ADDED: Composite index for filtered task queries (workspace_id, status, created_at DESC)
CREATE INDEX IF NOT EXISTS idx_tasks_workspace_status ON tasks(workspace_id, status, created_at DESC);
-- ADDED: Composite index for workspace task queries (workspace_id, created_at DESC)
CREATE INDEX IF NOT EXISTS idx_tasks_workspace_created ON tasks(workspace_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_tasks_status ON tasks(status);
CREATE INDEX IF NOT EXISTS idx_tasks_queued_at ON tasks(queued_at) WHERE queued_at IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_tasks_worker_id ON tasks(worker_id) WHERE worker_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_tasks_source_ids ON tasks USING GIN(source_ids);
CREATE INDEX IF NOT EXISTS idx_tasks_pr_status ON tasks(pr_status) WHERE pr_status IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_tasks_branch_name ON tasks(branch_name) WHERE branch_name IS NOT NULL;

-- Task-Project many-to-many relationship
CREATE TABLE IF NOT EXISTS task_projects (
  task_id UUID NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
  project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  created_at TIMESTAMP DEFAULT NOW(),
  PRIMARY KEY (task_id, project_id)
);

CREATE INDEX IF NOT EXISTS idx_task_projects_task_id ON task_projects(task_id);
CREATE INDEX IF NOT EXISTS idx_task_projects_project_id ON task_projects(project_id);

-- Task Queue
CREATE TABLE IF NOT EXISTS task_queue (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  task_id UUID NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
  priority INTEGER NOT NULL DEFAULT 3,
  queued_at TIMESTAMP DEFAULT NOW(),
  started_at TIMESTAMP,
  worker_id TEXT,
  attempts INTEGER DEFAULT 0,
  max_attempts INTEGER DEFAULT 3,
  last_error TEXT,
  UNIQUE(task_id)
);

CREATE INDEX IF NOT EXISTS idx_task_queue_priority ON task_queue(priority DESC, queued_at ASC);
CREATE INDEX IF NOT EXISTS idx_task_queue_worker ON task_queue(worker_id) WHERE worker_id IS NOT NULL;

-- Task Runs
CREATE TABLE IF NOT EXISTS task_runs (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  task_id UUID NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
  status TEXT NOT NULL CHECK (status IN ('running', 'completed', 'failed', 'cancelled')),
  current_phase TEXT,
  progress_percent INTEGER DEFAULT 0 CHECK (progress_percent >= 0 AND progress_percent <= 100),
  started_at TIMESTAMP DEFAULT NOW(),
  completed_at TIMESTAMP,
  error_message TEXT,
  artifacts JSONB DEFAULT '{}'::jsonb,
  result_summary TEXT,
  modified_files JSONB
);

-- REPLACED: idx_task_runs_task_id with composite index including sort order
CREATE INDEX IF NOT EXISTS idx_task_runs_task_started ON task_runs(task_id, started_at DESC);
CREATE INDEX IF NOT EXISTS idx_task_runs_status ON task_runs(status);

CREATE TABLE IF NOT EXISTS task_run_logs (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  task_run_id UUID NOT NULL REFERENCES task_runs(id) ON DELETE CASCADE,
  phase TEXT NOT NULL,
  agent_type TEXT NOT NULL,
  log_level TEXT NOT NULL CHECK (log_level IN ('debug', 'info', 'warning', 'error')),
  message TEXT NOT NULL,
  metadata JSONB DEFAULT '{}'::jsonb,
  created_at TIMESTAMP DEFAULT NOW()
);

-- REPLACED: idx_task_run_logs_run_id with composite index including sort order
CREATE INDEX IF NOT EXISTS idx_task_run_logs_run_created ON task_run_logs(task_run_id, created_at ASC);

CREATE TABLE IF NOT EXISTS task_tool_calls (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  task_run_id UUID NOT NULL REFERENCES task_runs(id) ON DELETE CASCADE,
  tool_name TEXT NOT NULL,
  tool_input JSONB NOT NULL,
  tool_output JSONB,
  status TEXT NOT NULL DEFAULT 'pending' CHECK (status IN ('pending', 'running', 'completed', 'failed')),
  error_message TEXT,
  started_at TIMESTAMP DEFAULT NOW(),
  completed_at TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_task_tool_calls_run_id ON task_tool_calls(task_run_id);
CREATE INDEX IF NOT EXISTS idx_task_tool_calls_status ON task_tool_calls(status);

CREATE TABLE IF NOT EXISTS task_file_changes (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  task_run_id UUID NOT NULL REFERENCES task_runs(id) ON DELETE CASCADE,
  file_path TEXT NOT NULL,
  change_type TEXT NOT NULL CHECK (change_type IN ('create', 'modify', 'delete')),
  original_content TEXT,
  new_content TEXT,
  diff TEXT,
  applied BOOLEAN DEFAULT FALSE,
  applied_at TIMESTAMP,
  reverted BOOLEAN DEFAULT FALSE,
  reverted_at TIMESTAMP,
  created_at TIMESTAMP DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_task_file_changes_run_id ON task_file_changes(task_run_id);
CREATE INDEX IF NOT EXISTS idx_task_file_changes_applied ON task_file_changes(applied);

-- =============================================================================
-- Chats
-- =============================================================================
CREATE TABLE IF NOT EXISTS chats (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  workspace_id UUID REFERENCES workspaces(id) ON DELETE CASCADE,
  title TEXT NOT NULL,
  model_name TEXT NOT NULL,
  created_at TIMESTAMP DEFAULT NOW(),
  updated_at TIMESTAMP DEFAULT NOW(),
  archived BOOLEAN DEFAULT FALSE
);

-- ADDED: Composite index for filtered chat queries (workspace_id, archived, updated_at DESC)
CREATE INDEX IF NOT EXISTS idx_chats_workspace_archived ON chats(workspace_id, archived, updated_at DESC);

CREATE TABLE IF NOT EXISTS messages (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  chat_id UUID NOT NULL REFERENCES chats(id) ON DELETE CASCADE,
  role TEXT NOT NULL CHECK (role IN ('user', 'assistant', 'system')),
  content TEXT NOT NULL,
  created_at TIMESTAMP DEFAULT NOW(),
  metadata JSONB DEFAULT '{}'::jsonb
);

-- REPLACED: idx_messages_chat_id with composite index including sort order
CREATE INDEX IF NOT EXISTS idx_messages_chat_created ON messages(chat_id, created_at ASC);

-- =============================================================================
-- Wiki
-- =============================================================================
CREATE TABLE IF NOT EXISTS wiki_entries (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  title TEXT NOT NULL,
  content TEXT NOT NULL,
  source_type TEXT NOT NULL CHECK (source_type IN
    ('chat', 'manual', 'url', 'task', 'github')),
  source_id UUID,
  source_url TEXT,
  created_at TIMESTAMP DEFAULT NOW(),
  updated_at TIMESTAMP DEFAULT NOW(),
  metadata JSONB DEFAULT '{}'::jsonb
);

CREATE INDEX IF NOT EXISTS idx_wiki_entries_source_type ON wiki_entries(source_type);

CREATE TABLE IF NOT EXISTS wiki_chunks (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  wiki_entry_id UUID NOT NULL REFERENCES wiki_entries(id) ON DELETE CASCADE,
  chunk_index INTEGER NOT NULL,
  content TEXT NOT NULL,
  embedding vector(1024),
  token_count INTEGER,
  created_at TIMESTAMP DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_wiki_chunks_entry_id ON wiki_chunks(wiki_entry_id);
CREATE INDEX IF NOT EXISTS idx_wiki_chunks_embedding ON wiki_chunks USING ivfflat (embedding vector_cosine_ops) WITH (lists = 100);

-- =============================================================================
-- Context & Embeddings
-- =============================================================================
CREATE TABLE IF NOT EXISTS content_items (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  source_id UUID NOT NULL REFERENCES sources(id) ON DELETE CASCADE,
  workspace_id UUID REFERENCES workspaces(id) ON DELETE CASCADE,
  category TEXT NOT NULL,
  uri TEXT NOT NULL,
  title TEXT NOT NULL,
  content TEXT,
  content_type TEXT NOT NULL DEFAULT 'text/plain',
  token_count INTEGER NOT NULL DEFAULT 0,
  metadata_only BOOLEAN NOT NULL DEFAULT FALSE,
  content_hash TEXT NOT NULL,
  metadata JSONB NOT NULL DEFAULT '{}'::jsonb,
  search_vector tsvector,
  modified_at TIMESTAMP,
  fetched_at TIMESTAMP NOT NULL DEFAULT NOW(),
  created_at TIMESTAMP DEFAULT NOW(),
  updated_at TIMESTAMP DEFAULT NOW(),
  UNIQUE(source_id, uri)
);

CREATE INDEX IF NOT EXISTS idx_content_items_source ON content_items(source_id);
CREATE INDEX IF NOT EXISTS idx_content_items_workspace ON content_items(workspace_id);
CREATE INDEX IF NOT EXISTS idx_content_items_category ON content_items(category);
CREATE INDEX IF NOT EXISTS idx_content_items_hash ON content_items(content_hash);
CREATE INDEX IF NOT EXISTS idx_content_items_fetched ON content_items(fetched_at);
CREATE INDEX IF NOT EXISTS idx_content_items_metadata ON content_items USING GIN(metadata);
CREATE INDEX IF NOT EXISTS idx_content_items_search ON content_items USING GIN(search_vector);

CREATE TABLE IF NOT EXISTS content_chunks (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  content_item_id UUID NOT NULL REFERENCES content_items(id) ON DELETE CASCADE,
  chunk_index INTEGER NOT NULL,
  text TEXT NOT NULL,
  token_count INTEGER NOT NULL,
  start_offset INTEGER NOT NULL,
  end_offset INTEGER NOT NULL,
  search_vector tsvector,
  created_at TIMESTAMP DEFAULT NOW(),
  UNIQUE(content_item_id, chunk_index)
);

CREATE INDEX IF NOT EXISTS idx_content_chunks_item ON content_chunks(content_item_id);
CREATE INDEX IF NOT EXISTS idx_content_chunks_search ON content_chunks USING GIN(search_vector);

CREATE TABLE IF NOT EXISTS embeddings (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  chunk_id UUID NOT NULL REFERENCES content_chunks(id) ON DELETE CASCADE UNIQUE,
  content_item_id UUID NOT NULL REFERENCES content_items(id) ON DELETE CASCADE,
  source_id UUID NOT NULL REFERENCES sources(id) ON DELETE CASCADE,
  workspace_id UUID REFERENCES workspaces(id) ON DELETE CASCADE,
  vector vector(1536) NOT NULL,
  model TEXT NOT NULL,
  created_at TIMESTAMP DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_embeddings_vector ON embeddings USING hnsw (vector vector_cosine_ops) WITH (m = 16, ef_construction = 64);
CREATE INDEX IF NOT EXISTS idx_embeddings_source ON embeddings(source_id);
CREATE INDEX IF NOT EXISTS idx_embeddings_content_item ON embeddings(content_item_id);
CREATE INDEX IF NOT EXISTS idx_embeddings_workspace ON embeddings(workspace_id);

CREATE TABLE IF NOT EXISTS heuristic_analysis (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  content_item_id UUID NOT NULL REFERENCES content_items(id) ON DELETE CASCADE UNIQUE,
  entities JSONB NOT NULL DEFAULT '{}'::jsonb,
  categorization JSONB NOT NULL DEFAULT '{}'::jsonb,
  quality JSONB NOT NULL DEFAULT '{}'::jsonb,
  analyzed_at TIMESTAMP NOT NULL DEFAULT NOW(),
  created_at TIMESTAMP DEFAULT NOW(),
  updated_at TIMESTAMP DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_heuristic_analysis_item ON heuristic_analysis(content_item_id);
CREATE INDEX IF NOT EXISTS idx_heuristic_analysis_entities ON heuristic_analysis USING GIN(entities);
CREATE INDEX IF NOT EXISTS idx_heuristic_analysis_categorization ON heuristic_analysis USING GIN(categorization);

CREATE TABLE IF NOT EXISTS message_embeddings (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  message_id UUID NOT NULL REFERENCES messages(id) ON DELETE CASCADE UNIQUE,
  chat_id UUID NOT NULL REFERENCES chats(id) ON DELETE CASCADE,
  vector vector(1536) NOT NULL,
  model TEXT NOT NULL,
  created_at TIMESTAMP DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_message_embeddings_vector ON message_embeddings USING hnsw (vector vector_cosine_ops) WITH (m = 16, ef_construction = 64);
CREATE INDEX IF NOT EXISTS idx_message_embeddings_chat ON message_embeddings(chat_id);

-- =============================================================================
-- Knowledge Base
-- =============================================================================
CREATE TABLE IF NOT EXISTS knowledge_entries (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  workspace_id UUID NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
  title TEXT NOT NULL,
  content TEXT NOT NULL,
  category TEXT,
  tags TEXT[] DEFAULT '{}',
  token_count INTEGER NOT NULL DEFAULT 0,
  is_active BOOLEAN DEFAULT TRUE,
  source_url TEXT,
  last_fetched_at TIMESTAMP,
  content_hash TEXT,
  refresh_interval_minutes INTEGER,
  last_fetch_error TEXT,
  created_by UUID REFERENCES users(id) ON DELETE SET NULL,
  created_at TIMESTAMP DEFAULT NOW(),
  updated_at TIMESTAMP DEFAULT NOW()
);

-- ADDED: Partial composite index for filtered knowledge queries (workspace_id, category, created_at DESC) WHERE is_active = TRUE
CREATE INDEX IF NOT EXISTS idx_knowledge_workspace_category ON knowledge_entries(workspace_id, category, created_at DESC) WHERE is_active = TRUE;
CREATE INDEX IF NOT EXISTS idx_knowledge_entries_tags ON knowledge_entries USING GIN(tags);
CREATE INDEX IF NOT EXISTS idx_knowledge_refresh_due ON knowledge_entries(workspace_id, source_url) WHERE source_url IS NOT NULL AND is_active = TRUE;
CREATE INDEX IF NOT EXISTS idx_knowledge_source_url ON knowledge_entries(workspace_id, source_url) WHERE source_url IS NOT NULL;

CREATE TABLE IF NOT EXISTS knowledge_embeddings (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  knowledge_entry_id UUID NOT NULL REFERENCES knowledge_entries(id) ON DELETE CASCADE UNIQUE,
  workspace_id UUID NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
  vector vector(1536) NOT NULL,
  model TEXT NOT NULL,
  created_at TIMESTAMP DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_knowledge_embeddings_vector ON knowledge_embeddings USING hnsw (vector vector_cosine_ops) WITH (m = 16, ef_construction = 64);
CREATE INDEX IF NOT EXISTS idx_knowledge_embeddings_workspace ON knowledge_embeddings(workspace_id);

-- =============================================================================
-- Context Gathering
-- =============================================================================
CREATE TABLE IF NOT EXISTS context_gatherings (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  workspace_id UUID REFERENCES workspaces(id) ON DELETE SET NULL,
  task_id UUID REFERENCES tasks(id) ON DELETE SET NULL,
  user_id UUID REFERENCES users(id) ON DELETE SET NULL,
  status TEXT NOT NULL DEFAULT 'pending' CHECK (status IN ('pending', 'running', 'completed', 'failed')),
  source_ids UUID[] NOT NULL DEFAULT '{}',
  config JSONB NOT NULL DEFAULT '{}'::jsonb,
  stats JSONB,
  error_message TEXT,
  started_at TIMESTAMP,
  completed_at TIMESTAMP,
  created_at TIMESTAMP DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_context_gatherings_workspace ON context_gatherings(workspace_id);
CREATE INDEX IF NOT EXISTS idx_context_gatherings_task ON context_gatherings(task_id);
CREATE INDEX IF NOT EXISTS idx_context_gatherings_user ON context_gatherings(user_id);
CREATE INDEX IF NOT EXISTS idx_context_gatherings_status ON context_gatherings(status);
-- ADDED: GIN index for source_ids array queries (for ANY() operations)
CREATE INDEX IF NOT EXISTS idx_context_gatherings_source_ids ON context_gatherings USING GIN(source_ids);

CREATE TABLE IF NOT EXISTS gathering_events (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  gathering_id UUID NOT NULL REFERENCES context_gatherings(id) ON DELETE CASCADE,
  event_type TEXT NOT NULL,
  payload JSONB NOT NULL DEFAULT '{}'::jsonb,
  created_at TIMESTAMP DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_gathering_events_gathering ON gathering_events(gathering_id);
CREATE INDEX IF NOT EXISTS idx_gathering_events_created ON gathering_events(created_at);
CREATE INDEX IF NOT EXISTS idx_gathering_events_gathering_created ON gathering_events(gathering_id, created_at);

-- =============================================================================
-- Workspace Settings
-- =============================================================================
CREATE TABLE IF NOT EXISTS workspace_themes (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  workspace_id UUID NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE UNIQUE,
  primary_color_light TEXT DEFAULT '#3b82f6',
  secondary_color_light TEXT DEFAULT '#6366f1',
  primary_color_dark TEXT DEFAULT '#3b82f6',
  secondary_color_dark TEXT DEFAULT '#6366f1',
  font_family TEXT DEFAULT 'system',
  font_size_base TEXT DEFAULT '16px',
  border_radius TEXT DEFAULT 'medium',
  created_at TIMESTAMP DEFAULT NOW(),
  updated_at TIMESTAMP DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_workspace_themes_workspace_id ON workspace_themes(workspace_id);

-- =============================================================================
-- AI Provider Settings
-- =============================================================================
CREATE TABLE IF NOT EXISTS organization_ai_settings (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  organization_id UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE UNIQUE,
  provider TEXT NOT NULL DEFAULT 'self_hosted' CHECK (provider IN ('self_hosted', 'openai', 'anthropic', 'bedrock')),
  litellm_host TEXT,
  litellm_key TEXT,
  openai_api_key TEXT,
  openai_base_url TEXT,
  anthropic_api_key TEXT,
  anthropic_base_url TEXT,
  bedrock_region TEXT,
  bedrock_access_key TEXT,
  bedrock_secret_key TEXT,
  bedrock_use_iam_role BOOLEAN DEFAULT FALSE,
  model_fast TEXT,
  model_reasoning TEXT,
  model_embedding TEXT,
  created_at TIMESTAMP DEFAULT NOW(),
  updated_at TIMESTAMP DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_org_ai_settings_org_id ON organization_ai_settings(organization_id);

CREATE TABLE IF NOT EXISTS workspace_ai_settings (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  workspace_id UUID NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE UNIQUE,
  provider TEXT CHECK (provider IS NULL OR provider IN ('self_hosted', 'openai', 'anthropic', 'bedrock')),
  litellm_host TEXT,
  litellm_key TEXT,
  openai_api_key TEXT,
  openai_base_url TEXT,
  anthropic_api_key TEXT,
  anthropic_base_url TEXT,
  bedrock_region TEXT,
  bedrock_access_key TEXT,
  bedrock_secret_key TEXT,
  bedrock_use_iam_role BOOLEAN,
  model_fast TEXT,
  model_reasoning TEXT,
  model_embedding TEXT,
  created_at TIMESTAMP DEFAULT NOW(),
  updated_at TIMESTAMP DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_ws_ai_settings_ws_id ON workspace_ai_settings(workspace_id);

-- =============================================================================
-- Billing & Subscriptions
-- =============================================================================
CREATE TABLE IF NOT EXISTS plans (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  name VARCHAR(100) NOT NULL,
  slug VARCHAR(50) NOT NULL UNIQUE,
  description TEXT,
  price_monthly_cents INTEGER NOT NULL,
  price_yearly_cents INTEGER NOT NULL,
  is_active BOOLEAN NOT NULL DEFAULT TRUE,
  is_public BOOLEAN NOT NULL DEFAULT TRUE,
  features JSONB NOT NULL DEFAULT '{}',
  limits JSONB NOT NULL DEFAULT '{}',
  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS subscriptions (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  organization_id UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
  plan_id UUID NOT NULL REFERENCES plans(id),
  status VARCHAR(50) NOT NULL,
  current_period_start TIMESTAMPTZ NOT NULL,
  current_period_end TIMESTAMPTZ NOT NULL,
  cancel_at_period_end BOOLEAN NOT NULL DEFAULT FALSE,
  canceled_at TIMESTAMPTZ,
  trial_start TIMESTAMPTZ,
  trial_end TIMESTAMPTZ,
  stripe_subscription_id VARCHAR(255),
  stripe_customer_id VARCHAR(255),
  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  UNIQUE(organization_id)
);

CREATE INDEX IF NOT EXISTS idx_subscriptions_status ON subscriptions(status);
CREATE INDEX IF NOT EXISTS idx_subscriptions_stripe ON subscriptions(stripe_subscription_id);

CREATE TABLE IF NOT EXISTS usage_events (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  organization_id UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
  workspace_id UUID REFERENCES workspaces(id) ON DELETE SET NULL,
  user_id UUID REFERENCES users(id) ON DELETE SET NULL,
  event_type VARCHAR(50) NOT NULL,
  quantity BIGINT NOT NULL DEFAULT 1,
  metadata JSONB DEFAULT '{}',
  recorded_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_usage_events_org_time ON usage_events(organization_id, recorded_at);
CREATE INDEX IF NOT EXISTS idx_usage_events_type ON usage_events(event_type);
CREATE INDEX IF NOT EXISTS idx_usage_events_org_type_time ON usage_events(organization_id, event_type, recorded_at);

-- =============================================================================
-- Audit Logs
-- =============================================================================
CREATE TABLE IF NOT EXISTS audit_logs (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  organization_id UUID REFERENCES organizations(id) ON DELETE SET NULL,
  workspace_id UUID REFERENCES workspaces(id) ON DELETE SET NULL,
  actor_id UUID REFERENCES users(id) ON DELETE SET NULL,
  actor_email VARCHAR(255),
  action VARCHAR(100) NOT NULL,
  resource_type VARCHAR(50) NOT NULL,
  resource_id UUID,
  old_values JSONB,
  new_values JSONB,
  ip_address INET,
  user_agent TEXT,
  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_audit_logs_org ON audit_logs(organization_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_audit_logs_actor ON audit_logs(actor_id);
CREATE INDEX IF NOT EXISTS idx_audit_logs_resource ON audit_logs(resource_type, resource_id);
CREATE INDEX IF NOT EXISTS idx_audit_logs_action ON audit_logs(action);
CREATE INDEX IF NOT EXISTS idx_audit_logs_created ON audit_logs(created_at DESC);

-- =============================================================================
-- External Sync
-- =============================================================================
CREATE TABLE IF NOT EXISTS sync_configs (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  provider TEXT NOT NULL CHECK (provider IN ('github', 'linear')),
  enabled BOOLEAN NOT NULL DEFAULT true,
  config JSONB NOT NULL DEFAULT '{}'::jsonb,
  webhook_secret_encrypted TEXT,
  created_at TIMESTAMP DEFAULT NOW(),
  updated_at TIMESTAMP DEFAULT NOW(),
  UNIQUE(project_id, provider)
);

CREATE INDEX IF NOT EXISTS idx_sync_configs_project_id ON sync_configs(project_id);
CREATE INDEX IF NOT EXISTS idx_sync_configs_provider ON sync_configs(provider);
CREATE INDEX IF NOT EXISTS idx_sync_configs_enabled ON sync_configs(enabled) WHERE enabled = true;

CREATE TABLE IF NOT EXISTS synced_items (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  sync_config_id UUID NOT NULL REFERENCES sync_configs(id) ON DELETE CASCADE,
  task_id UUID NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
  external_id TEXT NOT NULL,
  external_url TEXT,
  last_synced_at TIMESTAMP DEFAULT NOW(),
  sync_direction TEXT NOT NULL DEFAULT 'bidirectional' CHECK (sync_direction IN ('outbound', 'inbound', 'bidirectional')),
  last_external_state JSONB,
  created_at TIMESTAMP DEFAULT NOW(),
  UNIQUE(sync_config_id, task_id),
  UNIQUE(sync_config_id, external_id)
);

CREATE INDEX IF NOT EXISTS idx_synced_items_sync_config_id ON synced_items(sync_config_id);
CREATE INDEX IF NOT EXISTS idx_synced_items_task_id ON synced_items(task_id);
CREATE INDEX IF NOT EXISTS idx_synced_items_external_id ON synced_items(sync_config_id, external_id);
CREATE INDEX IF NOT EXISTS idx_synced_items_last_synced ON synced_items(last_synced_at);

CREATE TABLE IF NOT EXISTS sync_events (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  sync_config_id UUID NOT NULL REFERENCES sync_configs(id) ON DELETE CASCADE,
  synced_item_id UUID REFERENCES synced_items(id) ON DELETE SET NULL,
  event_type TEXT NOT NULL CHECK (event_type IN ('create', 'update', 'close', 'webhook_received', 'sync_error')),
  direction TEXT NOT NULL CHECK (direction IN ('outbound', 'inbound')),
  payload JSONB,
  error_message TEXT,
  created_at TIMESTAMP DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_sync_events_sync_config_id ON sync_events(sync_config_id);
CREATE INDEX IF NOT EXISTS idx_sync_events_synced_item_id ON sync_events(synced_item_id);
CREATE INDEX IF NOT EXISTS idx_sync_events_event_type ON sync_events(event_type);
CREATE INDEX IF NOT EXISTS idx_sync_events_created_at ON sync_events(created_at DESC);

-- =============================================================================
-- Functions & Triggers
-- =============================================================================

-- Task Queue Functions
CREATE OR REPLACE FUNCTION claim_next_task(p_worker_id TEXT)
RETURNS TABLE(task_id UUID, queue_id UUID) AS $$
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
$$ LANGUAGE plpgsql;

CREATE OR REPLACE FUNCTION release_task(p_task_id UUID, p_error TEXT DEFAULT NULL)
RETURNS VOID AS $$
BEGIN
  UPDATE task_queue SET worker_id = NULL, started_at = NULL, last_error = COALESCE(p_error, last_error)
  WHERE task_id = p_task_id;

  UPDATE tasks SET status = 'queued', worker_id = NULL, updated_at = NOW()
  WHERE id = p_task_id;
END;
$$ LANGUAGE plpgsql;

CREATE OR REPLACE FUNCTION complete_task_in_queue(p_task_id UUID, p_success BOOLEAN)
RETURNS VOID AS $$
BEGIN
  DELETE FROM task_queue WHERE task_id = p_task_id;

  UPDATE tasks
  SET status = CASE WHEN p_success THEN 'complete' ELSE 'blocked' END,
      worker_id = NULL,
      completed_at = CASE WHEN p_success THEN NOW() ELSE NULL END,
      updated_at = NOW()
  WHERE id = p_task_id;
END;
$$ LANGUAGE plpgsql;

CREATE OR REPLACE FUNCTION recover_orphaned_tasks()
RETURNS INTEGER AS $$
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
$$ LANGUAGE plpgsql;

-- Task Source Functions
CREATE OR REPLACE FUNCTION get_task_source(p_task_id UUID)
RETURNS TABLE(source_id UUID, source_type TEXT, config JSONB, credentials TEXT) AS $$
BEGIN
  RETURN QUERY
  SELECT t.source_id, s.source_type, s.config, s.credentials_encrypted
  FROM tasks t
  LEFT JOIN sources s ON s.id = t.source_id
  WHERE t.id = p_task_id AND s.is_active = TRUE;
END;
$$ LANGUAGE plpgsql;

CREATE OR REPLACE FUNCTION get_task_sources(p_task_id UUID)
RETURNS TABLE(source_id UUID, source_type TEXT, category TEXT, config JSONB, credentials TEXT) AS $$
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
$$ LANGUAGE plpgsql;

CREATE OR REPLACE FUNCTION get_task_sources_by_category(p_task_id UUID, p_category TEXT)
RETURNS TABLE(source_id UUID, source_type TEXT, config JSONB, credentials TEXT) AS $$
BEGIN
  RETURN QUERY
  SELECT ts.source_id, ts.source_type, ts.config, ts.credentials
  FROM get_task_sources(p_task_id) ts WHERE ts.category = p_category;
END;
$$ LANGUAGE plpgsql;

-- Search Functions
CREATE OR REPLACE FUNCTION search_content_embeddings(
  query_vector vector(1536),
  p_limit INTEGER DEFAULT 10,
  p_threshold FLOAT DEFAULT 0.7,
  p_source_ids UUID[] DEFAULT NULL,
  p_workspace_id UUID DEFAULT NULL,
  p_categories TEXT[] DEFAULT NULL
)
RETURNS TABLE(chunk_id UUID, content_item_id UUID, source_id UUID, similarity FLOAT, chunk_text TEXT, item_uri TEXT, item_title TEXT, item_metadata JSONB) AS $$
BEGIN
  RETURN QUERY
  SELECT e.chunk_id, e.content_item_id, e.source_id,
         (1 - (e.vector <=> query_vector))::FLOAT as similarity,
         cc.text as chunk_text, ci.uri as item_uri, ci.title as item_title, ci.metadata as item_metadata
  FROM embeddings e
  JOIN content_chunks cc ON cc.id = e.chunk_id
  JOIN content_items ci ON ci.id = e.content_item_id
  WHERE (1 - (e.vector <=> query_vector)) >= p_threshold
    AND (p_source_ids IS NULL OR e.source_id = ANY(p_source_ids))
    AND (p_workspace_id IS NULL OR e.workspace_id = p_workspace_id)
    AND (p_categories IS NULL OR ci.category = ANY(p_categories))
  ORDER BY e.vector <=> query_vector
  LIMIT p_limit;
END;
$$ LANGUAGE plpgsql;

CREATE OR REPLACE FUNCTION search_knowledge(
  query_vector vector(1536),
  p_workspace_id UUID,
  p_limit INTEGER DEFAULT 10,
  p_threshold FLOAT DEFAULT 0.7
)
RETURNS TABLE(entry_id UUID, similarity FLOAT, title TEXT, content TEXT, category TEXT, tags TEXT[]) AS $$
BEGIN
  RETURN QUERY
  SELECT ke.id as entry_id, (1 - (ke_embed.vector <=> query_vector))::FLOAT as similarity,
         ke.title, ke.content, ke.category, ke.tags
  FROM knowledge_entries ke
  JOIN knowledge_embeddings ke_embed ON ke_embed.knowledge_entry_id = ke.id
  WHERE ke.workspace_id = p_workspace_id AND ke.is_active = TRUE
    AND (1 - (ke_embed.vector <=> query_vector)) >= p_threshold
  ORDER BY ke_embed.vector <=> query_vector
  LIMIT p_limit;
END;
$$ LANGUAGE plpgsql;

CREATE OR REPLACE FUNCTION search_chat_history(
  query_vector vector(1536),
  p_chat_id UUID,
  p_limit INTEGER DEFAULT 10,
  p_threshold FLOAT DEFAULT 0.7
)
RETURNS TABLE(message_id UUID, similarity FLOAT, role TEXT, content TEXT, created_at TIMESTAMP) AS $$
BEGIN
  RETURN QUERY
  SELECT m.id as message_id, (1 - (me.vector <=> query_vector))::FLOAT as similarity,
         m.role, m.content, m.created_at
  FROM message_embeddings me
  JOIN messages m ON m.id = me.message_id
  WHERE me.chat_id = p_chat_id AND (1 - (me.vector <=> query_vector)) >= p_threshold
  ORDER BY me.vector <=> query_vector
  LIMIT p_limit;
END;
$$ LANGUAGE plpgsql;

-- Membership Helper Functions
CREATE OR REPLACE FUNCTION check_workspace_membership(p_user_id UUID, p_workspace_id UUID)
RETURNS BOOLEAN AS $$
BEGIN
  RETURN EXISTS(SELECT 1 FROM workspace_members WHERE user_id = p_user_id AND workspace_id = p_workspace_id AND is_active = TRUE);
END;
$$ LANGUAGE plpgsql;

CREATE OR REPLACE FUNCTION get_workspace_role(p_user_id UUID, p_workspace_id UUID)
RETURNS TEXT AS $$
DECLARE v_role TEXT;
BEGIN
  SELECT role INTO v_role FROM workspace_members WHERE user_id = p_user_id AND workspace_id = p_workspace_id AND is_active = TRUE;
  RETURN v_role;
END;
$$ LANGUAGE plpgsql;

CREATE OR REPLACE FUNCTION check_organization_membership(p_user_id UUID, p_organization_id UUID)
RETURNS BOOLEAN AS $$
BEGIN
  RETURN EXISTS(SELECT 1 FROM organization_members WHERE user_id = p_user_id AND organization_id = p_organization_id AND is_active = TRUE);
END;
$$ LANGUAGE plpgsql;

CREATE OR REPLACE FUNCTION get_organization_role(p_user_id UUID, p_organization_id UUID)
RETURNS TEXT AS $$
DECLARE v_role TEXT;
BEGIN
  SELECT role INTO v_role FROM organization_members WHERE user_id = p_user_id AND organization_id = p_organization_id AND is_active = TRUE;
  RETURN v_role;
END;
$$ LANGUAGE plpgsql;

-- Full-text Search Triggers
CREATE OR REPLACE FUNCTION update_chunk_search_vector()
RETURNS TRIGGER AS $$
BEGIN
  NEW.search_vector := to_tsvector('english', COALESCE(NEW.text, ''));
  RETURN NEW;
END;
$$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS chunk_search_vector_trigger ON content_chunks;
CREATE TRIGGER chunk_search_vector_trigger
  BEFORE INSERT OR UPDATE OF text ON content_chunks
  FOR EACH ROW EXECUTE FUNCTION update_chunk_search_vector();

CREATE OR REPLACE FUNCTION update_item_search_vector()
RETURNS TRIGGER AS $$
BEGIN
  NEW.search_vector := setweight(to_tsvector('english', COALESCE(NEW.title, '')), 'A') ||
                       setweight(to_tsvector('english', COALESCE(NEW.content, '')), 'B');
  RETURN NEW;
END;
$$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS item_search_vector_trigger ON content_items;
CREATE TRIGGER item_search_vector_trigger
  BEFORE INSERT OR UPDATE OF title, content ON content_items
  FOR EACH ROW EXECUTE FUNCTION update_item_search_vector();

CREATE OR REPLACE FUNCTION update_sync_configs_updated_at()
RETURNS TRIGGER AS $$
BEGIN
  NEW.updated_at = NOW();
  RETURN NEW;
END;
$$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS sync_configs_updated_at ON sync_configs;
CREATE TRIGGER sync_configs_updated_at
  BEFORE UPDATE ON sync_configs
  FOR EACH ROW EXECUTE FUNCTION update_sync_configs_updated_at();

-- =============================================================================
-- Seed Data
-- =============================================================================

-- Source Types
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

-- Permissions
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
  ('users:delete', 'Delete users', 'users', 'delete')
ON CONFLICT (name) DO NOTHING;

-- Roles
INSERT INTO roles (id, name, description, is_system) VALUES
  ('00000000-0000-0000-0000-000000000001', 'admin', 'Full system access', TRUE),
  ('00000000-0000-0000-0000-000000000002', 'user', 'Standard user access', TRUE),
  ('00000000-0000-0000-0000-000000000003', 'viewer', 'Read-only access', TRUE)
ON CONFLICT (id) DO NOTHING;

-- Role Permissions
INSERT INTO role_permissions (role_id, permission_id)
SELECT '00000000-0000-0000-0000-000000000001', id FROM permissions
ON CONFLICT DO NOTHING;

INSERT INTO role_permissions (role_id, permission_id)
SELECT '00000000-0000-0000-0000-000000000002', id FROM permissions
WHERE name IN (
  'projects:create', 'projects:read', 'projects:update', 'projects:delete',
  'tasks:create', 'tasks:read', 'tasks:update', 'tasks:delete',
  'chats:create', 'chats:read', 'chats:update', 'chats:delete',
  'sources:read', 'models:read', 'wiki:read', 'wiki:create', 'wiki:update'
)
ON CONFLICT DO NOTHING;

INSERT INTO role_permissions (role_id, permission_id)
SELECT '00000000-0000-0000-0000-000000000003', id FROM permissions WHERE action = 'read'
ON CONFLICT DO NOTHING;

-- Default Organization & Workspace
INSERT INTO organizations (id, name, slug, description, is_active) VALUES
  ('00000000-0000-0000-0000-000000000001', 'Default Organization', 'default', 'Default organization for self-hosted installation', true)
ON CONFLICT (id) DO NOTHING;

INSERT INTO workspaces (id, organization_id, name, slug, description, is_active) VALUES
  ('00000000-0000-0000-0000-000000000001', '00000000-0000-0000-0000-000000000001', 'Default Workspace', 'default', 'Default workspace for self-hosted installation', true)
ON CONFLICT (id) DO NOTHING;

-- Plans
INSERT INTO plans (name, slug, description, price_monthly_cents, price_yearly_cents, features, limits) VALUES
  ('Free', 'free', 'For individuals and small teams', 0, 0, '{"api_access": true}'::jsonb, '{"max_workspaces": 1, "max_members": 3, "max_chats_per_month": 100}'::jsonb),
  ('Pro', 'pro', 'For growing teams', 2900, 29000, '{"api_access": true, "priority_support": true}'::jsonb, '{"max_workspaces": 10, "max_members": 25, "max_chats_per_month": 5000}'::jsonb),
  ('Enterprise', 'enterprise', 'For large organizations', 9900, 99000, '{"api_access": true, "priority_support": true, "sso": true, "audit_log": true}'::jsonb, '{"max_workspaces": -1, "max_members": -1, "max_chats_per_month": -1}'::jsonb)
ON CONFLICT (slug) DO NOTHING;

-- Record migration
INSERT INTO schema_migrations (version) VALUES (1) ON CONFLICT DO NOTHING;

COMMIT;
