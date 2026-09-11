-- no-transaction
-- Admission uniqueness may never lapse, so the superset index is built first and
-- the 'running'-only one it replaces is dropped later (030). Exactly one
-- statement per file: a multi-statement simple query opens an implicit
-- transaction block, and CONCURRENTLY is rejected inside one with 25001.
CREATE UNIQUE INDEX CONCURRENTLY IF NOT EXISTS task_runs_active_waiting ON public.task_runs(task_id) WHERE status IN ('running', 'waiting');
