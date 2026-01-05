-- Extended Source Categories
-- Migration 005
-- Adds support for calendar, mail, chat, web, and text sources

BEGIN;

-- Add category column to source_types
ALTER TABLE source_types ADD COLUMN category TEXT NOT NULL DEFAULT 'file';

-- Update existing source types with their category
UPDATE source_types SET category = 'file' WHERE name IN ('github', 'gitlab', 'filesystem');

-- Insert new source types

-- Calendar sources
INSERT INTO source_types (name, category, description, config_schema) VALUES
  ('ical', 'calendar', 'iCalendar subscription URL', '{
    "type": "object",
    "required": ["url"],
    "properties": {
      "url": {"type": "string", "description": "iCal feed URL"},
      "refresh_interval": {"type": "integer", "description": "Refresh interval in seconds", "default": 3600}
    }
  }');

-- Mail sources
INSERT INTO source_types (name, category, description, config_schema) VALUES
  ('imap', 'mail', 'IMAP mail server', '{
    "type": "object",
    "required": ["host", "port", "username"],
    "properties": {
      "host": {"type": "string", "description": "IMAP server hostname"},
      "port": {"type": "integer", "description": "IMAP server port", "default": 993},
      "username": {"type": "string", "description": "IMAP username/email"},
      "use_ssl": {"type": "boolean", "description": "Use SSL/TLS connection", "default": true},
      "folder": {"type": "string", "description": "Default folder to access", "default": "INBOX"}
    }
  }');

-- Chat sources (future implementations)
INSERT INTO source_types (name, category, description, config_schema) VALUES
  ('discord', 'chat', 'Discord server', '{
    "type": "object",
    "required": ["server_id"],
    "properties": {
      "server_id": {"type": "string", "description": "Discord server/guild ID"},
      "channel_ids": {"type": "array", "items": {"type": "string"}, "description": "Specific channel IDs to access"}
    }
  }'),
  ('slack', 'chat', 'Slack workspace', '{
    "type": "object",
    "required": ["workspace_id"],
    "properties": {
      "workspace_id": {"type": "string", "description": "Slack workspace ID"},
      "channel_ids": {"type": "array", "items": {"type": "string"}, "description": "Specific channel IDs to access"}
    }
  }');

-- Simple sources
INSERT INTO source_types (name, category, description, config_schema) VALUES
  ('web', 'web', 'Web URL content fetcher', '{
    "type": "object",
    "required": ["url"],
    "properties": {
      "url": {"type": "string", "description": "URL to fetch content from"},
      "headers": {"type": "object", "description": "Custom HTTP headers to include"}
    }
  }'),
  ('text', 'text', 'Raw text/string content', '{
    "type": "object",
    "required": ["content"],
    "properties": {
      "content": {"type": "string", "description": "Raw text content"},
      "label": {"type": "string", "description": "Optional label for the content"}
    }
  }');

-- Multi-source support for tasks
-- Add source_ids array column (keeping source_id for backwards compatibility)
ALTER TABLE tasks ADD COLUMN source_ids UUID[] DEFAULT '{}';

-- Migrate existing source_id to source_ids array
UPDATE tasks
SET source_ids = ARRAY[source_id]::UUID[]
WHERE source_id IS NOT NULL AND (source_ids IS NULL OR source_ids = '{}');

-- Create index for efficient source lookups
CREATE INDEX idx_tasks_source_ids ON tasks USING GIN(source_ids);
CREATE INDEX idx_source_types_category ON source_types(category);

-- Update get_task_source function to support multiple sources
-- Returns all sources for a task (task sources + project source as fallback)
CREATE OR REPLACE FUNCTION get_task_sources(p_task_id UUID)
RETURNS TABLE(
  source_id UUID,
  source_type TEXT,
  category TEXT,
  config JSONB,
  credentials TEXT
) AS $$
BEGIN
  RETURN QUERY
  -- First get task-specific sources
  SELECT
    s.id as source_id,
    s.source_type,
    st.category,
    s.config,
    s.credentials_encrypted
  FROM tasks t
  CROSS JOIN LATERAL unnest(t.source_ids) AS task_source_id
  JOIN sources s ON s.id = task_source_id
  JOIN source_types st ON st.name = s.source_type
  WHERE t.id = p_task_id
    AND s.is_active = TRUE

  UNION

  -- Then add project source as fallback if no task sources
  SELECT
    s.id as source_id,
    s.source_type,
    st.category,
    s.config,
    s.credentials_encrypted
  FROM tasks t
  JOIN projects p ON p.id = t.project_id
  JOIN sources s ON s.id = p.source_id
  JOIN source_types st ON st.name = s.source_type
  WHERE t.id = p_task_id
    AND s.is_active = TRUE
    AND (t.source_ids IS NULL OR t.source_ids = '{}');
END;
$$ LANGUAGE plpgsql;

-- Function to get sources by category for a task
CREATE OR REPLACE FUNCTION get_task_sources_by_category(p_task_id UUID, p_category TEXT)
RETURNS TABLE(
  source_id UUID,
  source_type TEXT,
  config JSONB,
  credentials TEXT
) AS $$
BEGIN
  RETURN QUERY
  SELECT
    ts.source_id,
    ts.source_type,
    ts.config,
    ts.credentials
  FROM get_task_sources(p_task_id) ts
  WHERE ts.category = p_category;
END;
$$ LANGUAGE plpgsql;

-- Record migration
INSERT INTO schema_migrations (version) VALUES (5);

COMMIT;
