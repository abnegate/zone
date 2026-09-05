-- Per-chat character / persona for models that expect a card.
-- Official instruct models keep Zone's assistant prompt when this is null.

BEGIN;

ALTER TABLE chats
  ADD COLUMN IF NOT EXISTS character JSONB;

COMMIT;
