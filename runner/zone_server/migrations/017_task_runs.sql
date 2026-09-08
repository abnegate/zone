ALTER TABLE task_runs ADD COLUMN heartbeat_at TIMESTAMPTZ NOT NULL DEFAULT NOW();
ALTER TABLE task_runs ADD COLUMN owner UUID;
ALTER TABLE task_runs ADD COLUMN triggered_by UUID REFERENCES users(id) ON DELETE SET NULL;
ALTER TABLE tasks ADD COLUMN active_run_id UUID REFERENCES task_runs(id) ON DELETE SET NULL;

-- Keep the newest active run when upgrading databases that admitted duplicates.
WITH ranked AS (
    SELECT id, ROW_NUMBER() OVER (PARTITION BY task_id ORDER BY started_at DESC NULLS LAST, id DESC) AS position
    FROM task_runs WHERE status = 'running'
)
UPDATE task_runs SET status = 'failed', error_message = 'superseded', completed_at = NOW()
WHERE id IN (SELECT id FROM ranked WHERE position > 1);

UPDATE tasks SET active_run_id = task_runs.id, status = 'in_progress'
FROM task_runs WHERE task_runs.task_id = tasks.id AND task_runs.status = 'running';

CREATE UNIQUE INDEX task_runs_active ON task_runs(task_id) WHERE status = 'running';
CREATE INDEX task_runs_heartbeat ON task_runs(heartbeat_at) WHERE status = 'running';
