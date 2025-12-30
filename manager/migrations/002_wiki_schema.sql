-- Zone Manager Wiki Schema with pgvector
-- Migration 002

BEGIN;

-- Enable pgvector extension
CREATE EXTENSION IF NOT EXISTS vector;

-- Wiki Entries
CREATE TABLE wiki_entries (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  title TEXT NOT NULL,
  content TEXT NOT NULL,
  source_type TEXT NOT NULL CHECK (source_type IN
    ('chat', 'manual', 'url', 'task', 'github')),
  source_id UUID,
  source_url TEXT,
  created_at TIMESTAMP WITH TIME ZONE DEFAULT NOW(),
  updated_at TIMESTAMP WITH TIME ZONE DEFAULT NOW(),
  metadata JSONB DEFAULT '{}'::jsonb
);

CREATE INDEX idx_wiki_entries_source_type ON wiki_entries(source_type);

-- Wiki Chunks with vector embeddings
CREATE TABLE wiki_chunks (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  wiki_entry_id UUID NOT NULL REFERENCES wiki_entries(id) ON DELETE CASCADE,
  chunk_index INTEGER NOT NULL,
  content TEXT NOT NULL,
  embedding vector(1024),
  token_count INTEGER,
  created_at TIMESTAMP WITH TIME ZONE DEFAULT NOW()
);

CREATE INDEX idx_wiki_chunks_entry_id ON wiki_chunks(wiki_entry_id);

-- Vector similarity search index (IVFFlat for good balance of speed/recall)
CREATE INDEX idx_wiki_chunks_embedding ON wiki_chunks
  USING ivfflat (embedding vector_cosine_ops) WITH (lists = 100);

-- Record migration
INSERT INTO schema_migrations (version) VALUES (2);

COMMIT;
