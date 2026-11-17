-- no-transaction
CREATE INDEX CONCURRENTLY IF NOT EXISTS idx_tasks_workspace_pr_created
    ON tasks (workspace_id, pr_created_at) WHERE pr_url IS NOT NULL;
