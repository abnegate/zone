SET LOCAL lock_timeout = '5s';

ALTER TABLE tasks DROP CONSTRAINT tasks_status_check;
ALTER TABLE tasks ADD CONSTRAINT tasks_status_check CHECK (status = ANY (ARRAY['created', 'queued', 'in_progress', 'review', 'complete', 'blocked', 'paused', 'cancelled']::text[]));

ALTER TABLE tasks ADD COLUMN shipped_at timestamp without time zone;
UPDATE tasks SET shipped_at = completed_at WHERE status = 'complete' AND completed_at IS NOT NULL;

ALTER TABLE task_automation ADD COLUMN parked_stage text;
ALTER TABLE task_automation ADD COLUMN parked_reason text;
ALTER TABLE task_automation ADD CONSTRAINT task_automation_parked_check CHECK (stage = 'paused' OR (parked_stage IS NULL AND parked_reason IS NULL));
