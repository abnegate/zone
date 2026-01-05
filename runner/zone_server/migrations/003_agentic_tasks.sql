-- Agentic Tasks and Queue Persistence
-- Migration 003

BEGIN;

-- Add agentic task fields to tasks table
ALTER TABLE tasks ADD COLUMN is_agentic BOOLEAN NOT NULL DEFAULT FALSE;
ALTER TABLE tasks ADD COLUMN github_repo_url TEXT;
ALTER TABLE tasks ADD COLUMN queued_at TIMESTAMP;
ALTER TABLE tasks ADD COLUMN worker_id TEXT;

-- Index for finding queued tasks efficiently
CREATE INDEX idx_tasks_queued_at ON tasks(queued_at) WHERE queued_at IS NOT NULL;
CREATE INDEX idx_tasks_worker_id ON tasks(worker_id) WHERE worker_id IS NOT NULL;

-- Add artifacts storage to task_runs for agentic task outputs
ALTER TABLE task_runs ADD COLUMN IF NOT EXISTS artifacts JSONB DEFAULT '{}'::jsonb;

-- Table for tracking agent tool calls within a run
CREATE TABLE task_tool_calls (
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

CREATE INDEX idx_task_tool_calls_run_id ON task_tool_calls(task_run_id);
CREATE INDEX idx_task_tool_calls_status ON task_tool_calls(status);

-- Table for file changes made by agentic tasks (for review/rollback)
CREATE TABLE task_file_changes (
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

CREATE INDEX idx_task_file_changes_run_id ON task_file_changes(task_run_id);
CREATE INDEX idx_task_file_changes_applied ON task_file_changes(applied);

-- Queue management table for persistent task queue
CREATE TABLE task_queue (
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

CREATE INDEX idx_task_queue_priority ON task_queue(priority DESC, queued_at ASC);
CREATE INDEX idx_task_queue_worker ON task_queue(worker_id) WHERE worker_id IS NOT NULL;

-- Function to claim next task from queue
CREATE OR REPLACE FUNCTION claim_next_task(p_worker_id TEXT)
RETURNS TABLE(task_id UUID, queue_id UUID) AS $$
DECLARE
  v_queue_id UUID;
  v_task_id UUID;
BEGIN
  -- Get and lock next available task
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
    -- Claim the task
    UPDATE task_queue
    SET worker_id = p_worker_id,
        started_at = NOW(),
        attempts = attempts + 1
    WHERE id = v_queue_id;

    -- Update task status
    UPDATE tasks
    SET status = 'in_progress',
        worker_id = p_worker_id,
        started_at = COALESCE(started_at, NOW()),
        updated_at = NOW()
    WHERE id = v_task_id;

    RETURN QUERY SELECT v_task_id, v_queue_id;
  END IF;

  RETURN;
END;
$$ LANGUAGE plpgsql;

-- Function to release task back to queue (for graceful shutdown or failure)
CREATE OR REPLACE FUNCTION release_task(p_task_id UUID, p_error TEXT DEFAULT NULL)
RETURNS VOID AS $$
BEGIN
  -- Update queue entry
  UPDATE task_queue
  SET worker_id = NULL,
      started_at = NULL,
      last_error = COALESCE(p_error, last_error)
  WHERE task_id = p_task_id;

  -- Reset task status
  UPDATE tasks
  SET status = 'queued',
      worker_id = NULL,
      updated_at = NOW()
  WHERE id = p_task_id;
END;
$$ LANGUAGE plpgsql;

-- Function to complete task in queue
CREATE OR REPLACE FUNCTION complete_task_in_queue(p_task_id UUID, p_success BOOLEAN)
RETURNS VOID AS $$
BEGIN
  -- Remove from queue
  DELETE FROM task_queue WHERE task_id = p_task_id;

  -- Update task status
  UPDATE tasks
  SET status = CASE WHEN p_success THEN 'complete' ELSE 'blocked' END,
      worker_id = NULL,
      completed_at = CASE WHEN p_success THEN NOW() ELSE NULL END,
      updated_at = NOW()
  WHERE id = p_task_id;
END;
$$ LANGUAGE plpgsql;

-- Function to recover orphaned tasks on startup
CREATE OR REPLACE FUNCTION recover_orphaned_tasks()
RETURNS INTEGER AS $$
DECLARE
  v_count INTEGER;
BEGIN
  -- Release any tasks claimed by workers that are no longer running
  -- (worker_id is set but task hasn't been updated in 10 minutes)
  WITH orphaned AS (
    SELECT t.id
    FROM tasks t
    JOIN task_queue tq ON tq.task_id = t.id
    WHERE tq.worker_id IS NOT NULL
      AND t.status = 'in_progress'
      AND t.updated_at < NOW() - INTERVAL '10 minutes'
  )
  UPDATE task_queue tq
  SET worker_id = NULL,
      started_at = NULL,
      last_error = 'Worker timeout - task recovered'
  FROM orphaned o
  WHERE tq.task_id = o.id;

  GET DIAGNOSTICS v_count = ROW_COUNT;

  -- Also reset task status for orphaned tasks
  UPDATE tasks
  SET status = 'queued',
      worker_id = NULL,
      updated_at = NOW()
  WHERE id IN (
    SELECT task_id FROM task_queue WHERE worker_id IS NULL
  )
  AND status = 'in_progress';

  RETURN v_count;
END;
$$ LANGUAGE plpgsql;

-- Record migration
INSERT INTO schema_migrations (version) VALUES (3);

COMMIT;
