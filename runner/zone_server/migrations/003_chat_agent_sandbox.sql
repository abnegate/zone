-- Sandbox toggle for agentic chat
-- Chooses between read-only workspace tools and the full host tool set.

BEGIN;

-- Sandboxed by default: the host tools write files and run commands in a
-- working directory, so leaving the sandbox has to be a deliberate choice
-- rather than something a chat inherits by being switched into agent mode.
ALTER TABLE chats
  ADD COLUMN IF NOT EXISTS agent_sandboxed BOOLEAN NOT NULL DEFAULT TRUE;

COMMIT;
