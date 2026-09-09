-- no-transaction
CREATE INDEX CONCURRENTLY IF NOT EXISTS task_runs_heartbeat ON public.task_runs(heartbeat_at) WHERE status = 'running';
