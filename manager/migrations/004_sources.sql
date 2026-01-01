-- Sources Abstraction
-- Migration 004
-- Supports GitHub, GitLab, and local filesystem sources

BEGIN;

-- Source types enum-like table for extensibility
CREATE TABLE source_types (
  name TEXT PRIMARY KEY,
  description TEXT NOT NULL,
  config_schema JSONB NOT NULL DEFAULT '{}'::jsonb,
  created_at TIMESTAMP WITH TIME ZONE DEFAULT NOW()
);

-- Insert supported source types with their config schemas
INSERT INTO source_types (name, description, config_schema) VALUES
  ('github', 'GitHub repository', '{
    "type": "object",
    "required": ["owner", "repo"],
    "properties": {
      "owner": {"type": "string", "description": "Repository owner/organization"},
      "repo": {"type": "string", "description": "Repository name"},
      "branch": {"type": "string", "description": "Default branch", "default": "main"},
      "base_path": {"type": "string", "description": "Path prefix within repo", "default": ""}
    }
  }'),
  ('gitlab', 'GitLab repository', '{
    "type": "object",
    "required": ["project_id"],
    "properties": {
      "project_id": {"type": "string", "description": "GitLab project ID or path"},
      "host": {"type": "string", "description": "GitLab host URL", "default": "https://gitlab.com"},
      "branch": {"type": "string", "description": "Default branch", "default": "main"},
      "base_path": {"type": "string", "description": "Path prefix within repo", "default": ""}
    }
  }'),
  ('filesystem', 'Local filesystem (self-hosted only)', '{
    "type": "object",
    "required": ["base_path"],
    "properties": {
      "base_path": {"type": "string", "description": "Absolute path to project root"},
      "allow_writes": {"type": "boolean", "description": "Allow write operations", "default": true}
    }
  }');

-- Sources table
CREATE TABLE sources (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  name TEXT NOT NULL,
  source_type TEXT NOT NULL REFERENCES source_types(name),
  config JSONB NOT NULL DEFAULT '{}'::jsonb,
  -- Encrypted credentials (token, API key, etc.)
  credentials_encrypted TEXT,
  -- Display info
  description TEXT,
  url TEXT,
  -- Status
  is_active BOOLEAN DEFAULT TRUE,
  last_verified_at TIMESTAMP WITH TIME ZONE,
  last_error TEXT,
  -- Timestamps
  created_at TIMESTAMP WITH TIME ZONE DEFAULT NOW(),
  updated_at TIMESTAMP WITH TIME ZONE DEFAULT NOW(),
  -- Unique name per source type
  UNIQUE(name, source_type)
);

CREATE INDEX idx_sources_type ON sources(source_type);
CREATE INDEX idx_sources_active ON sources(is_active) WHERE is_active = TRUE;

-- Add source_id to projects (nullable for backwards compatibility)
ALTER TABLE projects ADD COLUMN source_id UUID REFERENCES sources(id) ON DELETE SET NULL;

-- Add source_id to tasks (for task-specific source override)
ALTER TABLE tasks ADD COLUMN source_id UUID REFERENCES sources(id) ON DELETE SET NULL;

-- Migrate existing github_repo_url data to sources
-- This creates a source for each unique GitHub URL found
DO $$
DECLARE
  r RECORD;
  v_source_id UUID;
  v_owner TEXT;
  v_repo TEXT;
BEGIN
  -- Migrate project GitHub URLs
  FOR r IN
    SELECT DISTINCT github_repo_url
    FROM projects
    WHERE github_repo_url IS NOT NULL AND github_repo_url != ''
  LOOP
    -- Parse owner/repo from URL (handles https://github.com/owner/repo format)
    v_owner := (regexp_match(r.github_repo_url, 'github\.com[/:]([^/]+)/([^/.]+)'))[1];
    v_repo := (regexp_match(r.github_repo_url, 'github\.com[/:]([^/]+)/([^/.]+)'))[2];

    IF v_owner IS NOT NULL AND v_repo IS NOT NULL THEN
      INSERT INTO sources (name, source_type, config, url, description)
      VALUES (
        v_owner || '/' || v_repo,
        'github',
        jsonb_build_object('owner', v_owner, 'repo', v_repo, 'branch', 'main'),
        r.github_repo_url,
        'Migrated from project'
      )
      ON CONFLICT (name, source_type) DO UPDATE SET updated_at = NOW()
      RETURNING id INTO v_source_id;

      -- Update projects using this URL
      UPDATE projects SET source_id = v_source_id WHERE github_repo_url = r.github_repo_url;
    END IF;
  END LOOP;

  -- Migrate task GitHub URLs
  FOR r IN
    SELECT DISTINCT github_repo_url
    FROM tasks
    WHERE github_repo_url IS NOT NULL AND github_repo_url != ''
  LOOP
    v_owner := (regexp_match(r.github_repo_url, 'github\.com[/:]([^/]+)/([^/.]+)'))[1];
    v_repo := (regexp_match(r.github_repo_url, 'github\.com[/:]([^/]+)/([^/.]+)'))[2];

    IF v_owner IS NOT NULL AND v_repo IS NOT NULL THEN
      INSERT INTO sources (name, source_type, config, url, description)
      VALUES (
        v_owner || '/' || v_repo,
        'github',
        jsonb_build_object('owner', v_owner, 'repo', v_repo, 'branch', 'main'),
        r.github_repo_url,
        'Migrated from task'
      )
      ON CONFLICT (name, source_type) DO UPDATE SET updated_at = NOW()
      RETURNING id INTO v_source_id;

      -- Update tasks using this URL
      UPDATE tasks SET source_id = v_source_id WHERE github_repo_url = r.github_repo_url;
    END IF;
  END LOOP;
END $$;

-- Now we can drop the old columns (keeping for rollback safety in production)
-- ALTER TABLE projects DROP COLUMN github_repo_url;
-- ALTER TABLE projects DROP COLUMN github_access_token;
-- ALTER TABLE tasks DROP COLUMN github_repo_url;

-- Function to get effective source for a task (task source or project source)
CREATE OR REPLACE FUNCTION get_task_source(p_task_id UUID)
RETURNS TABLE(
  source_id UUID,
  source_type TEXT,
  config JSONB,
  credentials TEXT
) AS $$
BEGIN
  RETURN QUERY
  SELECT
    COALESCE(t.source_id, p.source_id) as source_id,
    s.source_type,
    s.config,
    s.credentials_encrypted
  FROM tasks t
  JOIN projects p ON p.id = t.project_id
  LEFT JOIN sources s ON s.id = COALESCE(t.source_id, p.source_id)
  WHERE t.id = p_task_id
    AND s.is_active = TRUE;
END;
$$ LANGUAGE plpgsql;

-- Record migration
INSERT INTO schema_migrations (version) VALUES (4);

COMMIT;
