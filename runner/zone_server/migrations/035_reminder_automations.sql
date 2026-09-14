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

-- A timing mode this build does not know would be dispatched as an
-- exact_schedule, which is the wrong contract for a watch, so the column is
-- closed at the database rather than at the tool alone.
ALTER TABLE reminders DROP CONSTRAINT IF EXISTS reminders_timing_mode_check;
ALTER TABLE reminders ADD CONSTRAINT reminders_timing_mode_check
    CHECK (timing_mode IN ('exact_schedule', 'flexible_schedule', 'condition_watch'));

-- A condition_watch samples state at each firing and reports a difference, so
-- one that never fires again is not a watch. The rule is stated here as well as
-- in the tool because a row that reaches the worker without it would sample
-- once and report nothing for ever.
ALTER TABLE reminders DROP CONSTRAINT IF EXISTS reminders_watch_recurs_check;
ALTER TABLE reminders ADD CONSTRAINT reminders_watch_recurs_check
    CHECK (timing_mode <> 'condition_watch' OR rrule IS NOT NULL);

-- A recurring reminder ends by exhausting its rule or by outliving its
-- lifetime, and neither is a delivery. 'expired' is that ending, so a reader
-- can tell a schedule that ran out from one somebody cancelled.
ALTER TABLE reminders DROP CONSTRAINT IF EXISTS reminders_status_check;
ALTER TABLE reminders ADD CONSTRAINT reminders_status_check
    CHECK (status IN ('pending', 'delivered', 'cancelled', 'expired'));
