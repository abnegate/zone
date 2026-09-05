-- Native width for qwen3-embedding:0.6b (1024). Existing 1536-d rows cannot
-- be recast, so drop vectors and let the resync worker rebuild them.

DROP INDEX IF EXISTS idx_embeddings_vector;
DROP INDEX IF EXISTS idx_message_embeddings_vector;
DROP INDEX IF EXISTS idx_knowledge_embeddings_vector;

DELETE FROM embeddings;
DELETE FROM message_embeddings;
DELETE FROM knowledge_embeddings;

ALTER TABLE embeddings ALTER COLUMN vector TYPE vector(1024);
ALTER TABLE message_embeddings ALTER COLUMN vector TYPE vector(1024);
ALTER TABLE knowledge_embeddings ALTER COLUMN vector TYPE vector(1024);

CREATE INDEX IF NOT EXISTS idx_embeddings_vector
    ON embeddings USING hnsw (vector vector_cosine_ops) WITH (m = 16, ef_construction = 64);
CREATE INDEX IF NOT EXISTS idx_message_embeddings_vector
    ON message_embeddings USING hnsw (vector vector_cosine_ops) WITH (m = 16, ef_construction = 64);
CREATE INDEX IF NOT EXISTS idx_knowledge_embeddings_vector
    ON knowledge_embeddings USING hnsw (vector vector_cosine_ops) WITH (m = 16, ef_construction = 64);

DO $$
DECLARE
  fn record;
BEGIN
  FOR fn IN
    SELECT p.oid::regprocedure AS sig
    FROM pg_proc p
    JOIN pg_namespace n ON n.oid = p.pronamespace
    WHERE n.nspname = 'public'
      AND p.proname IN ('search_content_embeddings', 'search_knowledge', 'search_chat_history')
  LOOP
    EXECUTE 'DROP FUNCTION IF EXISTS ' || fn.sig;
  END LOOP;
END $$;

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
  SELECT e.chunk_id, e.content_item_id, e.source_id,
         (1 - (e.vector <=> query_vector))::FLOAT as similarity,
         cc.text as chunk_text, ci.uri as item_uri, ci.title as item_title, ci.metadata as item_metadata
  FROM embeddings e
  JOIN content_chunks cc ON cc.id = e.chunk_id
  JOIN content_items ci ON ci.id = e.content_item_id
  WHERE (1 - (e.vector <=> query_vector)) >= p_threshold
    AND (p_source_ids IS NULL OR e.source_id = ANY(p_source_ids))
    AND (p_workspace_id IS NULL OR e.workspace_id = p_workspace_id)
    AND (p_categories IS NULL OR ci.category = ANY(p_categories))
  ORDER BY e.vector <=> query_vector
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
  SELECT ke.id as entry_id, (1 - (ke_embed.vector <=> query_vector))::FLOAT as similarity,
         ke.title, ke.content, ke.category, ke.tags
  FROM knowledge_entries ke
  JOIN knowledge_embeddings ke_embed ON ke_embed.knowledge_entry_id = ke.id
  WHERE ke.workspace_id = p_workspace_id AND ke.is_active = TRUE
    AND (1 - (ke_embed.vector <=> query_vector)) >= p_threshold
  ORDER BY ke_embed.vector <=> query_vector
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
  SELECT m.id as message_id, (1 - (me.vector <=> query_vector))::FLOAT as similarity,
         m.role, m.content, m.created_at
  FROM message_embeddings me
  JOIN messages m ON m.id = me.message_id
  WHERE me.chat_id = p_chat_id AND (1 - (me.vector <=> query_vector)) >= p_threshold
  ORDER BY me.vector <=> query_vector
  LIMIT p_limit;
END;
$$ LANGUAGE plpgsql;

UPDATE organization_ai_settings
SET model_embedding = 'qwen3-embedding:0.6b'
WHERE model_embedding IS NULL OR model_embedding LIKE 'nomic-embed%';

UPDATE workspace_ai_settings
SET model_embedding = 'qwen3-embedding:0.6b'
WHERE model_embedding IS NULL OR model_embedding LIKE 'nomic-embed%';
