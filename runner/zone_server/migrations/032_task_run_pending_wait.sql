-- What a run is parked on waiting belongs beside the question it is parked on
-- asking: the writer holding the run's lease is the only one that sets either
-- and the only one that clears them, so the row lock already orders park and
-- resume. Kept separate from pending_question so answering a run that is
-- waiting on a job stays a conflict. Metadata only -- no scan, no backfill.
SET LOCAL lock_timeout = '5s';
ALTER TABLE task_runs ADD COLUMN IF NOT EXISTS pending_wait JSONB;
