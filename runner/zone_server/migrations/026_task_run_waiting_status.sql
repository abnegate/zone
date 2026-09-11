-- 'waiting' is a run that still holds its lease and its admission slot while a
-- workspace member answers a question. The inline CHECK from the initial schema
-- was auto-named task_runs_status_check by Postgres; re-adding it NOT VALID
-- widens the set without scanning the table, and 029 validates it once the
-- concurrent index builds that must not share its lock have finished.
SET LOCAL lock_timeout = '5s';
ALTER TABLE task_runs DROP CONSTRAINT IF EXISTS task_runs_status_check;
ALTER TABLE task_runs ADD CONSTRAINT task_runs_status_check
    CHECK (status IN ('running', 'waiting', 'completed', 'failed', 'cancelled')) NOT VALID;
