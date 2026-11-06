SET LOCAL lock_timeout = '5s';

ALTER TABLE public.chat_pull_requests
    ADD COLUMN auto_fix boolean NOT NULL DEFAULT false,
    -- The newest run auto-fix started. Holding it keeps a status read that
    -- repeats every poll from starting a second run while that one works.
    ADD COLUMN auto_fix_run_id uuid,
    -- Who turned auto-fix on: its runs act, and push, as them.
    ADD COLUMN auto_fix_by uuid,
    ADD CONSTRAINT chat_pull_requests_auto_fix_run_id_fkey
        FOREIGN KEY (auto_fix_run_id) REFERENCES public.task_runs(id) ON DELETE SET NULL NOT VALID,
    ADD CONSTRAINT chat_pull_requests_auto_fix_by_fkey
        FOREIGN KEY (auto_fix_by) REFERENCES public.users(id) ON DELETE SET NULL NOT VALID;
