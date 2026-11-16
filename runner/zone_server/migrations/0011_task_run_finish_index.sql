SET LOCAL lock_timeout = '5s';

CREATE INDEX idx_task_runs_finished ON task_runs ((COALESCE(completed_at, started_at)));
