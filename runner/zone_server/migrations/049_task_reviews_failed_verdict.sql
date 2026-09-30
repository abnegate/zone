-- 'failed' is a review round whose reviewer model errored before it answered.
-- Recording it moves the next round to another reviewer instead of asking the
-- same one again every tick. The inline CHECK from 042 was auto-named
-- task_reviews_verdict_check by Postgres.
SET LOCAL lock_timeout = '5s';
ALTER TABLE task_reviews DROP CONSTRAINT IF EXISTS task_reviews_verdict_check;
ALTER TABLE task_reviews ADD CONSTRAINT task_reviews_verdict_check
    CHECK (verdict IN ('approve', 'request_changes', 'unparseable', 'failed'));
