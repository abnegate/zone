-- The scan 038 deliberately did not do. VALIDATE CONSTRAINT takes SHARE UPDATE
-- EXCLUSIVE rather than ACCESS EXCLUSIVE, so reminders stay insertable and
-- updatable while it runs, which is the whole reason 038 added these NOT VALID.
-- Bounded here the way 036 bounds its own: the wait queues behind any
-- conflicting lock while the boot holds sqlx's advisory lock, so every other
-- instance is blocked behind this one.
--
-- All three hold over every existing row by construction. timing_mode has only
-- ever held 'exact_schedule', which the widened check still accepts, and which
-- the watch check passes without reading either of the columns it names.
-- last_observation is new in 038 and is NULL on every row that predates it. A
-- failure here means a row arrived from outside this build.
SET LOCAL lock_timeout = '5s';
ALTER TABLE reminders VALIDATE CONSTRAINT reminders_timing_mode_check;
ALTER TABLE reminders VALIDATE CONSTRAINT reminders_watch_compares_check;
ALTER TABLE reminders VALIDATE CONSTRAINT reminders_last_observation_check;
