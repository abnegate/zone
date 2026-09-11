-- no-transaction
-- The sweeper reads this index to find dead leases. A parked run keeps
-- heartbeating, so it has to be swept on the same terms as a running one or a
-- dead worker holds the task's admission slot forever.
CREATE INDEX CONCURRENTLY IF NOT EXISTS task_runs_heartbeat_waiting ON public.task_runs(heartbeat_at) WHERE status IN ('running', 'waiting');
