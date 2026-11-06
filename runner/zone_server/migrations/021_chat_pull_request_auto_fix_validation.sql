SET LOCAL lock_timeout = '5s';
ALTER TABLE public.chat_pull_requests VALIDATE CONSTRAINT chat_pull_requests_auto_fix_run_id_fkey;
ALTER TABLE public.chat_pull_requests VALIDATE CONSTRAINT chat_pull_requests_auto_fix_by_fkey;
