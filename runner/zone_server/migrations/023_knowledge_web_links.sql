-- Add web link support to knowledge entries
-- Allows users to add URLs that are automatically fetched and re-indexed

-- Add source URL field for web links
ALTER TABLE knowledge_entries ADD COLUMN IF NOT EXISTS source_url TEXT;

-- Add last fetched timestamp for tracking refresh
ALTER TABLE knowledge_entries ADD COLUMN IF NOT EXISTS last_fetched_at TIMESTAMP;

-- Add content hash for change detection (avoid re-embedding unchanged content)
ALTER TABLE knowledge_entries ADD COLUMN IF NOT EXISTS content_hash TEXT;

-- Add refresh interval in minutes (NULL = no auto-refresh, 0 = manual only)
ALTER TABLE knowledge_entries ADD COLUMN IF NOT EXISTS refresh_interval_minutes INTEGER;

-- Add fetch error tracking
ALTER TABLE knowledge_entries ADD COLUMN IF NOT EXISTS last_fetch_error TEXT;

-- Index for finding entries that need refresh
-- Entries with source_url, active, and (never fetched OR past refresh interval)
CREATE INDEX IF NOT EXISTS idx_knowledge_refresh_due ON knowledge_entries(workspace_id, source_url)
WHERE source_url IS NOT NULL AND is_active = TRUE;

-- Index for querying by URL (for deduplication within workspace)
CREATE INDEX IF NOT EXISTS idx_knowledge_source_url ON knowledge_entries(workspace_id, source_url)
WHERE source_url IS NOT NULL;
