-- Per-chat thinking depth. Auto inspects the next user request; Off skips
-- thinking even when the deployment advertised it.

BEGIN;

ALTER TABLE chats
  ADD COLUMN IF NOT EXISTS reasoning_effort TEXT NOT NULL DEFAULT 'auto';

ALTER TABLE chats
  DROP CONSTRAINT IF EXISTS chats_reasoning_effort_check;

ALTER TABLE chats
  ADD CONSTRAINT chats_reasoning_effort_check
  CHECK (reasoning_effort IN ('auto', 'off', 'low', 'medium', 'high'));

COMMIT;
