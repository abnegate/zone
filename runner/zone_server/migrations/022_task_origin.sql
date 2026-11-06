SET LOCAL lock_timeout = '5s';

ALTER TABLE public.tasks
    ADD COLUMN origin text DEFAULT 'member' NOT NULL,
    ADD CONSTRAINT tasks_origin_check CHECK (
        origin = ANY (ARRAY['member'::text, 'reminder'::text, 'automation'::text])
    ) NOT VALID;

UPDATE public.tasks AS task
SET origin = 'automation'
WHERE task.created_by IS NULL
   OR EXISTS (
       SELECT 1 FROM public.task_automation AS automation
       WHERE automation.task_id = task.id AND automation.auto_created
   );
