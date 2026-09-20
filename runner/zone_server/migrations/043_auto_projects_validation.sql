-- The scan 042 deliberately did not do. VALIDATE takes SHARE UPDATE EXCLUSIVE,
-- so chats and projects stay writable while it runs, and every constraint
-- holds by construction: purpose defaults to 'assistant' and both new
-- foreign-key columns are NULL on every existing row.
SET LOCAL lock_timeout = '5s';
ALTER TABLE chats VALIDATE CONSTRAINT chats_purpose_check;
ALTER TABLE chats VALIDATE CONSTRAINT chats_project_id_fkey;
ALTER TABLE projects VALIDATE CONSTRAINT projects_auto_actor_id_fkey;
