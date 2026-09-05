-- Auto-approve mutating file and shell tools in an agentic chat.
-- Off by default: writes and commands wait for a confirmation unless the
-- reader turns this on for the conversation.

BEGIN;

ALTER TABLE chats
  ADD COLUMN IF NOT EXISTS auto_approve BOOLEAN NOT NULL DEFAULT FALSE;

COMMIT;
