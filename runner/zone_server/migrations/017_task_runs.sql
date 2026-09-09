-- Metadata changes only: do not hold these locks through scans or backfills.
SET LOCAL lock_timeout = '5s';
ALTER TABLE task_runs ADD COLUMN heartbeat_at TIMESTAMPTZ NOT NULL DEFAULT NOW();
ALTER TABLE task_runs ADD COLUMN owner UUID;
ALTER TABLE task_runs ADD COLUMN triggered_by UUID;
ALTER TABLE tasks ADD COLUMN active_run_id UUID;
ALTER TABLE task_runs ADD CONSTRAINT task_runs_triggered_by_fkey
    FOREIGN KEY (triggered_by) REFERENCES users(id) ON DELETE SET NULL NOT VALID;
ALTER TABLE tasks ADD CONSTRAINT tasks_active_run_id_fkey
    FOREIGN KEY (active_run_id) REFERENCES task_runs(id) ON DELETE SET NULL NOT VALID;

-- Old binaries do not participate in admission locking. Pause only new running
-- admissions until reconciliation and the concurrent unique index are durable.
-- Existing runs may finish and unrelated writes continue during this upgrade.
CREATE FUNCTION guard_task_run_upgrade() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF NEW.status = 'running' AND
       (TG_OP = 'INSERT' OR OLD.status IS DISTINCT FROM 'running' OR OLD.task_id IS DISTINCT FROM NEW.task_id) THEN
        RAISE EXCEPTION 'Task admission paused for schema upgrade' USING ERRCODE = '55000';
    END IF;
    RETURN NEW;
END;
$$;
CREATE TRIGGER task_runs_migration_admission
    BEFORE INSERT OR UPDATE OF status, task_id ON task_runs
    FOR EACH ROW EXECUTE FUNCTION guard_task_run_upgrade();
