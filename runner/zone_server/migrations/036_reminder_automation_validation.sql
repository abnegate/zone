-- The scan 035 deliberately did not do. VALIDATE CONSTRAINT takes SHARE UPDATE
-- EXCLUSIVE rather than ACCESS EXCLUSIVE, so reminders stay insertable and
-- updatable while it runs, which is the whole reason 035 added these NOT VALID.
-- Bounded here the way 029 bounds its own: the wait queues behind any
-- conflicting lock while the boot holds sqlx's advisory lock, so every other
-- instance is blocked behind this one.
--
-- Both constraints hold over every existing row by construction: timing_mode
-- defaults to 'exact_schedule' and 035 is the only thing that has ever written
-- it, and status has only ever held the three values 006 allowed, which the
-- widened check still accepts. The validation is therefore expected to pass on
-- any database, and a failure here means a row arrived from outside this build.
SET LOCAL lock_timeout = '5s';
ALTER TABLE reminders VALIDATE CONSTRAINT reminders_timing_mode_check;
ALTER TABLE reminders VALIDATE CONSTRAINT reminders_status_check;
