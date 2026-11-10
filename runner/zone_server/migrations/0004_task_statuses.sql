SET LOCAL lock_timeout = '5s';

ALTER TABLE tasks DROP CONSTRAINT tasks_status_check;
ALTER TABLE tasks ADD CONSTRAINT tasks_status_check CHECK (status = ANY (ARRAY['created', 'queued', 'in_progress', 'review', 'complete', 'blocked', 'paused', 'cancelled']::text[]));
