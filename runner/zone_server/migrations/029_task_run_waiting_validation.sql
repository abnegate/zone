-- VALIDATE CONSTRAINT takes SHARE UPDATE EXCLUSIVE, the same lock the builds in
-- 027 and 028 hold, so this scan runs only once both are durable. It queues
-- behind any conflicting lock, and the boot holds sqlx's advisory lock while it
-- waits, so the wait is bounded here the way 025 and 026 bound theirs: every
-- other instance is blocked behind this one.
--
-- The guard asserts existence and validity, not a definition string:
-- pg_get_indexdef renders an IN predicate as = ANY (ARRAY[...]), so matching it
-- exactly is a trap rather than a check. IF NOT EXISTS in the two builds skips
-- an invalid leftover from a cancelled build, which db::migrations::repair drops
-- before those builds run; reaching this guard means an index went invalid some
-- other way, and the repair skips a migration already recorded.
SET LOCAL lock_timeout = '5s';
ALTER TABLE task_runs VALIDATE CONSTRAINT task_runs_status_check;

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_index
        WHERE indexrelid = to_regclass('public.task_runs_active_waiting') AND indisvalid
    ) OR NOT EXISTS (
        SELECT 1 FROM pg_index
        WHERE indexrelid = to_regclass('public.task_runs_heartbeat_waiting') AND indisvalid
    ) THEN
        RAISE EXCEPTION 'Task waiting indexes are incomplete; drop any invalid task_runs_active_waiting or task_runs_heartbeat_waiting index, delete the _sqlx_migrations rows for versions 27 and 28 so their builds re-run, and restart the server migration runner';
    END IF;
END;
$$;
