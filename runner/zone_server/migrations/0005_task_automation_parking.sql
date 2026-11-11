SET LOCAL lock_timeout = '5s';

ALTER TABLE task_automation ADD COLUMN parked_stage text;
ALTER TABLE task_automation ADD COLUMN parked_reason text;
ALTER TABLE task_automation ADD CONSTRAINT task_automation_parked_check CHECK (stage = 'paused' OR (parked_stage IS NULL AND parked_reason IS NULL));
