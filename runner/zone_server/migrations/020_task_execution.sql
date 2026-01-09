-- Task Execution Integration
-- Migration 020

BEGIN;

-- Add execution fields to task_runs if they don't exist
ALTER TABLE task_runs ADD COLUMN IF NOT EXISTS result_summary TEXT;

-- Record migration
INSERT INTO schema_migrations (version) VALUES (20);

COMMIT;
