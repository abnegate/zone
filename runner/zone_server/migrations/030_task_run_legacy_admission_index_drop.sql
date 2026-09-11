-- no-transaction
-- Superseded by task_runs_active_waiting, whose predicate is a strict superset,
-- and dropped only after 029 proved that index valid. DROP INDEX CONCURRENTLY
-- takes one name and refuses to run inside a transaction block.
DROP INDEX CONCURRENTLY IF EXISTS public.task_runs_active;
