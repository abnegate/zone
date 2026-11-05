SET LOCAL lock_timeout = '5s';

ALTER TABLE public.tasks
    ADD COLUMN title_pending boolean DEFAULT false NOT NULL;
