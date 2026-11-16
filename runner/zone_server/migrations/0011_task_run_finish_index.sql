SET LOCAL lock_timeout = '5s';

CREATE INDEX idx_task_runs_finished ON task_runs ((COALESCE(completed_at, started_at)));
CREATE INDEX idx_tasks_workspace_pr_created ON tasks (workspace_id, pr_created_at) WHERE pr_url IS NOT NULL;
