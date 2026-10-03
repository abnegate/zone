ALTER TABLE public.chats
    ADD COLUMN offline boolean DEFAULT false NOT NULL;
