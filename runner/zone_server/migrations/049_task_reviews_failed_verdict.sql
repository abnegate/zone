SET LOCAL lock_timeout = '5s';
ALTER TABLE task_reviews DROP CONSTRAINT IF EXISTS task_reviews_verdict_check;
ALTER TABLE task_reviews ADD CONSTRAINT task_reviews_verdict_check
    CHECK (verdict IN ('approve', 'request_changes', 'unparseable', 'failed')) NOT VALID;
