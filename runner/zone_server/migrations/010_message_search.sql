-- Chat history keyword search must use a stored GIN index, not
-- to_tsvector('english', content) evaluated on every query.

ALTER TABLE messages
  ADD COLUMN IF NOT EXISTS search_vector tsvector;

UPDATE messages
SET search_vector = to_tsvector('english', COALESCE(content, ''))
WHERE search_vector IS NULL;

CREATE OR REPLACE FUNCTION update_message_search_vector()
RETURNS trigger AS $$
BEGIN
  NEW.search_vector := to_tsvector('english', COALESCE(NEW.content, ''));
  RETURN NEW;
END;
$$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS message_search_vector_trigger ON messages;
CREATE TRIGGER message_search_vector_trigger
  BEFORE INSERT OR UPDATE OF content ON messages
  FOR EACH ROW
  EXECUTE FUNCTION update_message_search_vector();

CREATE INDEX IF NOT EXISTS idx_messages_search_vector
  ON messages USING GIN (search_vector);
