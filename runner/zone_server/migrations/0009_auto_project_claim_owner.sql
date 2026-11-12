SET LOCAL lock_timeout = '5s';

ALTER TABLE projects ADD COLUMN auto_claim_id uuid;
