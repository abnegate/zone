-- A task may ask that a run's plan be approved before the run changes
-- anything. The flag is the task's; the plan is the run's, kept on the row
-- once approval was asked for so a reviewer can read what was agreed to after
-- the question that carried it has been answered and cleared. Both are
-- metadata only -- no scan, no backfill -- and the run column is nullable
-- because most runs are never asked for one.
SET LOCAL lock_timeout = '5s';
ALTER TABLE tasks ADD COLUMN IF NOT EXISTS require_plan_approval BOOLEAN NOT NULL DEFAULT FALSE;
ALTER TABLE task_runs ADD COLUMN IF NOT EXISTS plan TEXT;
