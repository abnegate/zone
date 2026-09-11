-- VALIDATE CONSTRAINT takes SHARE UPDATE EXCLUSIVE, the same lock the builds in
-- 027 and 028 hold, so this scan runs only once both are durable.
--
-- The guard asserts existence and validity, not a definition string:
-- pg_get_indexdef renders an IN predicate as = ANY (ARRAY[...]), so matching it
-- exactly is a trap rather than a check. IF NOT EXISTS in the two builds skips
-- an invalid leftover from a cancelled build, which is what this catches.
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
        RAISE EXCEPTION 'Task waiting indexes are incomplete; drop any invalid task_runs_active_waiting or task_runs_heartbeat_waiting index and re-run the migration';
    END IF;
END;
$$;
