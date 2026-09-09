-- no-transaction
CREATE UNIQUE INDEX CONCURRENTLY IF NOT EXISTS task_runs_active ON public.task_runs(task_id) WHERE status = 'running';
