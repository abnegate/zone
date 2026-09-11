-- no-transaction
-- Superseded by task_runs_heartbeat_waiting, the same way 030 supersedes the
-- admission index.
DROP INDEX CONCURRENTLY IF EXISTS public.task_runs_heartbeat;
