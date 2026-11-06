SET LOCAL lock_timeout = '5s';

ALTER TABLE public.chat_pull_requests
    ADD COLUMN auto_fix boolean NOT NULL DEFAULT false,
    -- The run started for the newest state worth fixing. Holding it keeps a
    -- status read that repeats every poll from starting the same run again,
    -- and lets a finished run be followed by a new one when the state moves.
    ADD COLUMN auto_fix_run_id uuid,
    ADD CONSTRAINT chat_pull_requests_auto_fix_run_id_fkey
        FOREIGN KEY (auto_fix_run_id) REFERENCES public.task_runs(id) ON DELETE SET NULL NOT VALID;
