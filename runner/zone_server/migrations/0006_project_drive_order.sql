SET LOCAL lock_timeout = '5s';

ALTER TABLE projects ADD COLUMN auto_driven_at timestamp with time zone;
