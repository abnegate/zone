-- no-transaction
CREATE INDEX CONCURRENTLY IF NOT EXISTS idx_task_runs_finished
    ON task_runs ((COALESCE(completed_at, started_at)));
