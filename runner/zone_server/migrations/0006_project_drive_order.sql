SET LOCAL lock_timeout = '5s';

ALTER TABLE projects ADD COLUMN auto_driven_at timestamp with time zone;
ALTER TABLE projects ADD COLUMN auto_stalled boolean NOT NULL DEFAULT false;
UPDATE projects SET auto_stalled = TRUE WHERE auto_paused_reason LIKE 'nothing can start%';
