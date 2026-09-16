-- A reminder becomes an automation when it carries a rule for its own next
-- firing. Everything here is metadata on a table whose every existing row is a
-- one-shot: rrule NULL is exactly what a one-shot is, so no backfill exists and
-- no existing reminder changes behaviour.
--
-- anchor_at is the first firing, kept apart from due_at because due_at moves to
-- the next occurrence on every fire and the recurrence has to be measured from
-- a fixed point. Without it, "every second Monday" would drift a period each
-- time a firing was late.
--
-- expires_at is the seven-day lifetime: a schedule nobody renews is a schedule
-- nobody wanted. It is a column rather than a computed bound so a renewal is
-- one write, and so a reminder that has outlived it can be found by the same
-- index that finds a due one.
SET LOCAL lock_timeout = '5s';

ALTER TABLE reminders ADD COLUMN IF NOT EXISTS rrule TEXT;
ALTER TABLE reminders ADD COLUMN IF NOT EXISTS prompt TEXT;
ALTER TABLE reminders ADD COLUMN IF NOT EXISTS timing_mode TEXT NOT NULL DEFAULT 'exact_schedule';
ALTER TABLE reminders ADD COLUMN IF NOT EXISTS anchor_at TIMESTAMPTZ;
ALTER TABLE reminders ADD COLUMN IF NOT EXISTS expires_at TIMESTAMPTZ;
ALTER TABLE reminders ADD COLUMN IF NOT EXISTS fired_count INTEGER NOT NULL DEFAULT 0;
ALTER TABLE reminders ADD COLUMN IF NOT EXISTS last_fired_at TIMESTAMPTZ;

-- Every constraint below is added NOT VALID and validated in 036, the way 026
-- widens task_runs_status_check and 029 validates it. An ADD CONSTRAINT that
-- validates scans the whole table under ACCESS EXCLUSIVE, and the installer
-- runs this file in one transaction, so the scan would block every insert and
-- update to reminders until it committed. lock_timeout bounds how long the lock
-- is waited for, not how long it is held.

-- One mode, because one is what the worker dispatches: deliver_next fires at the
-- stated time and nowhere else. Accepting a mode the worker would run under a
-- different contract is worse than refusing it, so this widens when the worker
-- learns the other two rather than ahead of them.
ALTER TABLE reminders DROP CONSTRAINT IF EXISTS reminders_timing_mode_check;
ALTER TABLE reminders ADD CONSTRAINT reminders_timing_mode_check
    CHECK (timing_mode = 'exact_schedule') NOT VALID;

-- A recurring reminder ends by exhausting its rule or by outliving its
-- lifetime, and neither is a delivery. 'expired' is that ending, so a reader
-- can tell a schedule that ran out from one somebody cancelled.
ALTER TABLE reminders DROP CONSTRAINT IF EXISTS reminders_status_check;
ALTER TABLE reminders ADD CONSTRAINT reminders_status_check
    CHECK (status IN ('pending', 'delivered', 'cancelled', 'expired')) NOT VALID;
