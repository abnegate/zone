SET LOCAL lock_timeout = '5s';
ALTER TABLE chats VALIDATE CONSTRAINT chats_provider_check;
ALTER TABLE chats VALIDATE CONSTRAINT chats_provider_model_check;
ALTER TABLE chats VALIDATE CONSTRAINT chats_reasoning_effort_check;
