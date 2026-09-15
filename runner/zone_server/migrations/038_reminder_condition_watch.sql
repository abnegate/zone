-- A watch is a schedule that reports a difference rather than a time. Everything
-- it needs beyond what 035 already stores is one column and three checks.
--
-- last_observation is what the previous firing found, kept so the next one has
-- something to compare against. It is a column rather than a reading of the
-- chat, even though every firing lands in the same chat and the previous
-- answer is therefore already in the history the model is handed. That shortcut
-- breaks on compaction, and breaks quietly: an hourly watch outlives its own
-- baseline within a day, and afterwards the model is asked to compare against
-- something no longer in front of it and has no way to say so. It would report
-- the summary losing detail as a change and call it news. A watch that cries
-- wolf once a day is worse than one nobody was offered.
SET LOCAL lock_timeout = '5s';

ALTER TABLE reminders ADD COLUMN IF NOT EXISTS last_observation TEXT;

-- Every constraint below is added NOT VALID and validated in 039, the way 035
-- adds what 036 validates. An ADD CONSTRAINT that validates scans the whole
-- table under ACCESS EXCLUSIVE, and the installer runs this file in one
-- transaction, so the scan would block every insert and update to reminders
-- until it committed. lock_timeout bounds how long the lock is waited for, not
-- how long it is held.

-- The worker dispatches two modes now. flexible_schedule still needs a window
-- to place a firing inside, and placing one needs a reading of the person's day
-- that nothing here takes, so it stays out until the worker can honour it.
ALTER TABLE reminders DROP CONSTRAINT IF EXISTS reminders_timing_mode_check;
ALTER TABLE reminders ADD CONSTRAINT reminders_timing_mode_check
    CHECK (timing_mode IN ('exact_schedule', 'condition_watch')) NOT VALID;

-- A watch needs both halves of a comparison. Without an rrule it fires once and
-- has nothing to compare against; without a prompt it delivers fixed words and
-- has nothing to compare. Either way it is a reminder wearing a watch's name,
-- and the mode is refused rather than stored and quietly downgraded.
--
-- Blank counts as missing, not just NULL. `create` trims both and stores NULL
-- for an empty one, so the two agree on what "carries a prompt" means; written
-- out here because this constraint is the boundary a write that never went
-- through `create` still has to cross, and a watch holding a prompt of spaces
-- would fire for ever with nothing to ask.
--
-- "Has a character that is not whitespace" rather than a trim, because
-- `btrim` with one argument strips spaces and only spaces: a prompt of one tab
-- survives it and reads as present. The regular expression is what `trim` means
-- on the Rust side of the same check.
ALTER TABLE reminders DROP CONSTRAINT IF EXISTS reminders_watch_compares_check;
ALTER TABLE reminders ADD CONSTRAINT reminders_watch_compares_check
    CHECK (
        timing_mode <> 'condition_watch'
        OR (COALESCE(rrule, '') ~ '[^[:space:]]' AND COALESCE(prompt, '') ~ '[^[:space:]]')
    )
    NOT VALID;

-- A model's answer has no length anybody promised, and this one is read back
-- into the next firing's prompt. The worker truncates to this bound before
-- writing, so the constraint is what stops a later writer storing a baseline
-- that would crowd out the context it is meant to be compared in. Truncation is
-- safe where compaction was not: it takes the same leading characters every
-- firing, so two readings stay comparable, and what falls past the bound is
-- missed consistently rather than intermittently.
ALTER TABLE reminders DROP CONSTRAINT IF EXISTS reminders_last_observation_check;
ALTER TABLE reminders ADD CONSTRAINT reminders_last_observation_check
    CHECK (last_observation IS NULL OR length(last_observation) <= 4000) NOT VALID;
