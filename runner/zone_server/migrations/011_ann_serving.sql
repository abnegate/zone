-- 100M-doc serving: quantized ANN first stage, workspace-local keys,
-- and stored GIN for knowledge. First-stage search must not join the
-- document body until after a LIMIT on an index.

ALTER TABLE embeddings
  ADD COLUMN IF NOT EXISTS vector_bit bit(1024)
  GENERATED ALWAYS AS (binary_quantize(vector)::bit(1024)) STORED;

ALTER TABLE message_embeddings
  ADD COLUMN IF NOT EXISTS vector_bit bit(1024)
  GENERATED ALWAYS AS (binary_quantize(vector)::bit(1024)) STORED;

ALTER TABLE knowledge_embeddings
  ADD COLUMN IF NOT EXISTS vector_bit bit(1024)
  GENERATED ALWAYS AS (binary_quantize(vector)::bit(1024)) STORED;

ALTER TABLE message_embeddings
  ADD COLUMN IF NOT EXISTS workspace_id UUID REFERENCES workspaces(id) ON DELETE CASCADE;

UPDATE message_embeddings me
SET workspace_id = c.workspace_id
FROM chats c
WHERE me.chat_id = c.id
  AND me.workspace_id IS NULL;

CREATE INDEX IF NOT EXISTS idx_embeddings_workspace_source
  ON embeddings (workspace_id, source_id);

CREATE INDEX IF NOT EXISTS idx_embeddings_vector_bit
  ON embeddings USING hnsw (vector_bit bit_hamming_ops)
  WITH (m = 16, ef_construction = 64);

CREATE INDEX IF NOT EXISTS idx_message_embeddings_workspace_chat
  ON message_embeddings (workspace_id, chat_id);

CREATE INDEX IF NOT EXISTS idx_message_embeddings_vector_bit
  ON message_embeddings USING hnsw (vector_bit bit_hamming_ops)
  WITH (m = 16, ef_construction = 64);

CREATE INDEX IF NOT EXISTS idx_knowledge_embeddings_vector_bit
  ON knowledge_embeddings USING hnsw (vector_bit bit_hamming_ops)
  WITH (m = 16, ef_construction = 64);

ALTER TABLE knowledge_entries
  ADD COLUMN IF NOT EXISTS search_vector tsvector;

UPDATE knowledge_entries
SET search_vector = to_tsvector('english', title || ' ' || COALESCE(content, ''))
WHERE search_vector IS NULL;

CREATE OR REPLACE FUNCTION update_knowledge_search_vector()
RETURNS trigger AS $$
BEGIN
  NEW.search_vector := to_tsvector('english', COALESCE(NEW.title, '') || ' ' || COALESCE(NEW.content, ''));
  RETURN NEW;
END;
$$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS knowledge_search_vector_trigger ON knowledge_entries;
CREATE TRIGGER knowledge_search_vector_trigger
  BEFORE INSERT OR UPDATE OF title, content ON knowledge_entries
  FOR EACH ROW
  EXECUTE FUNCTION update_knowledge_search_vector();

CREATE INDEX IF NOT EXISTS idx_knowledge_entries_search_vector
  ON knowledge_entries USING GIN (search_vector)
  WHERE is_active = TRUE;

CREATE OR REPLACE FUNCTION search_content_embeddings(
  query_vector vector(1024),
  p_limit INTEGER DEFAULT 10,
  p_threshold FLOAT DEFAULT 0.7,
  p_source_ids UUID[] DEFAULT NULL,
  p_workspace_id UUID DEFAULT NULL,
  p_categories TEXT[] DEFAULT NULL
)
RETURNS TABLE(chunk_id UUID, content_item_id UUID, source_id UUID, similarity FLOAT, chunk_text TEXT, item_uri TEXT, item_title TEXT, item_metadata JSONB) AS $$
BEGIN
  RETURN QUERY
  WITH ann AS (
    SELECT e.chunk_id, e.content_item_id, e.source_id, e.vector
    FROM embeddings e
    WHERE (p_workspace_id IS NULL OR e.workspace_id = p_workspace_id)
      AND (p_source_ids IS NULL OR e.source_id = ANY(p_source_ids))
    ORDER BY e.vector_bit <~> binary_quantize(query_vector)::bit(1024)
    LIMIT GREATEST(p_limit * 8, 32)
  )
  SELECT ann.chunk_id, ann.content_item_id, ann.source_id,
         (1 - (ann.vector <=> query_vector))::FLOAT as similarity,
         cc.text as chunk_text, ci.uri as item_uri, ci.title as item_title, ci.metadata as item_metadata
  FROM ann
  JOIN content_chunks cc ON cc.id = ann.chunk_id
  JOIN content_items ci ON ci.id = ann.content_item_id
  WHERE (1 - (ann.vector <=> query_vector)) >= p_threshold
    AND (p_categories IS NULL OR ci.category = ANY(p_categories))
  ORDER BY ann.vector <=> query_vector
  LIMIT p_limit;
END;
$$ LANGUAGE plpgsql;

CREATE OR REPLACE FUNCTION search_knowledge(
  query_vector vector(1024),
  p_workspace_id UUID,
  p_limit INTEGER DEFAULT 10,
  p_threshold FLOAT DEFAULT 0.7
)
RETURNS TABLE(entry_id UUID, similarity FLOAT, title TEXT, content TEXT, category TEXT, tags TEXT[]) AS $$
BEGIN
  RETURN QUERY
  WITH ann AS (
    SELECT ke_embed.knowledge_entry_id, ke_embed.vector
    FROM knowledge_embeddings ke_embed
    WHERE ke_embed.workspace_id = p_workspace_id
    ORDER BY ke_embed.vector_bit <~> binary_quantize(query_vector)::bit(1024)
    LIMIT GREATEST(p_limit * 8, 32)
  )
  SELECT ke.id as entry_id, (1 - (ann.vector <=> query_vector))::FLOAT as similarity,
         ke.title, ke.content, ke.category, ke.tags
  FROM ann
  JOIN knowledge_entries ke ON ke.id = ann.knowledge_entry_id
  WHERE ke.is_active = TRUE
    AND (1 - (ann.vector <=> query_vector)) >= p_threshold
  ORDER BY ann.vector <=> query_vector
  LIMIT p_limit;
END;
$$ LANGUAGE plpgsql;

CREATE OR REPLACE FUNCTION search_chat_history(
  query_vector vector(1024),
  p_chat_id UUID,
  p_limit INTEGER DEFAULT 10,
  p_threshold FLOAT DEFAULT 0.7
)
RETURNS TABLE(message_id UUID, similarity FLOAT, role TEXT, content TEXT, created_at TIMESTAMP) AS $$
BEGIN
  RETURN QUERY
  WITH ann AS (
    SELECT me.message_id, me.vector
    FROM message_embeddings me
    WHERE me.chat_id = p_chat_id
    ORDER BY me.vector_bit <~> binary_quantize(query_vector)::bit(1024)
    LIMIT GREATEST(p_limit * 8, 32)
  )
  SELECT m.id as message_id, (1 - (ann.vector <=> query_vector))::FLOAT as similarity,
         m.role, m.content, m.created_at
  FROM ann
  JOIN messages m ON m.id = ann.message_id
  WHERE (1 - (ann.vector <=> query_vector)) >= p_threshold
  ORDER BY ann.vector <=> query_vector
  LIMIT p_limit;
END;
$$ LANGUAGE plpgsql;

DO $$
BEGIN
  PERFORM set_config('hnsw.iterative_scan', 'relaxed_order', false);
  PERFORM set_config('hnsw.ef_search', '80', false);
  PERFORM set_config('hnsw.max_scan_tuples', '20000', false);
EXCEPTION WHEN OTHERS THEN
  RAISE NOTICE 'hnsw session GUCs not available: %', SQLERRM;
END $$;

ANALYZE embeddings;
ANALYZE message_embeddings;
ANALYZE knowledge_embeddings;
ANALYZE content_chunks;
ANALYZE knowledge_entries;
