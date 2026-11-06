SET LOCAL lock_timeout = '5s';

ALTER TABLE public.tasks
    ADD COLUMN origin text DEFAULT 'member' NOT NULL,
    ADD CONSTRAINT tasks_origin_check CHECK (
        origin = ANY (ARRAY['member'::text, 'reminder'::text, 'automation'::text])
    ) NOT VALID;
