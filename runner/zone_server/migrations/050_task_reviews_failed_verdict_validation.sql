SET LOCAL lock_timeout = '5s';
ALTER TABLE task_reviews VALIDATE CONSTRAINT task_reviews_verdict_check;
