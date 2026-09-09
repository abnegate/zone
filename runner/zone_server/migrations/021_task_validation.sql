-- Validation scans run after metadata locks have been committed. These locks
-- permit ordinary writes; admission stays fenced until this transaction commits.
SET LOCAL lock_timeout = '5s';
ALTER TABLE task_runs VALIDATE CONSTRAINT task_runs_triggered_by_fkey;
ALTER TABLE tasks VALIDATE CONSTRAINT tasks_active_run_id_fkey;

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_index WHERE indexrelid = to_regclass('public.task_runs_active')
        AND indisvalid AND pg_get_indexdef(indexrelid) =
            'CREATE UNIQUE INDEX task_runs_active ON public.task_runs USING btree (task_id) WHERE (status = ''running''::text)'
    ) OR NOT EXISTS (
        SELECT 1 FROM pg_index WHERE indexrelid = to_regclass('public.task_runs_heartbeat')
        AND indisvalid AND pg_get_indexdef(indexrelid) =
            'CREATE INDEX task_runs_heartbeat ON public.task_runs USING btree (heartbeat_at) WHERE (status = ''running''::text)'
    ) THEN
        RAISE EXCEPTION 'Task indexes are incomplete; restart the server migration runner to repair them';
    END IF;
END;
$$;
DROP TRIGGER task_runs_migration_admission ON task_runs;
DROP FUNCTION guard_task_run_upgrade();
