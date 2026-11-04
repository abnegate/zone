SET LOCAL lock_timeout = '5s';

ALTER TABLE public.reminders
    ADD COLUMN time_zone text DEFAULT 'UTC'::text NOT NULL;

ALTER TABLE public.users
    ADD COLUMN time_zone text;
