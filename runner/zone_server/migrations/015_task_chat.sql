SET LOCAL lock_timeout = '5s';

ALTER TABLE public.tasks
    ADD COLUMN chat_id uuid,
    ADD CONSTRAINT tasks_chat_id_fkey FOREIGN KEY (chat_id) REFERENCES public.chats(id) ON DELETE SET NULL;

CREATE INDEX tasks_chat_id_idx
    ON public.tasks USING btree (chat_id, created_at DESC)
    WHERE chat_id IS NOT NULL;
