-- A question the run is parked on belongs with the run, not in a side table:
-- the writer that holds the run's lease is the only one that sets it and the
-- only one that clears it, so a single row lock already orders park, answer and
-- resume against each other. Metadata only -- no scan, no backfill.
SET LOCAL lock_timeout = '5s';
ALTER TABLE task_runs ADD COLUMN IF NOT EXISTS pending_question JSONB;
