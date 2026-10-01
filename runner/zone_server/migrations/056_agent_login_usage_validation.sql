SET LOCAL lock_timeout = '5s';
ALTER TABLE chats VALIDATE CONSTRAINT chats_agent_login_id_fkey;
