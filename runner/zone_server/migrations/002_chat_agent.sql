-- Agentic chat
-- Lets a conversation run a tool-calling loop instead of a single completion.

BEGIN;

-- Off by default: tool calling needs a model that supports it, and existing
-- chats were created against whatever model happened to be selected.
ALTER TABLE chats
  ADD COLUMN IF NOT EXISTS agent_enabled BOOLEAN NOT NULL DEFAULT FALSE;

COMMIT;
