-- Add full-text search support for hybrid retrieval
-- Combines PostgreSQL tsvector with existing pgvector semantic search

-- Add tsvector column to content_chunks for full-text search
ALTER TABLE content_chunks ADD COLUMN IF NOT EXISTS search_vector tsvector;

-- Create GIN index for fast full-text search
CREATE INDEX IF NOT EXISTS idx_content_chunks_search ON content_chunks USING GIN(search_vector);

-- Create function to update search_vector on insert/update
CREATE OR REPLACE FUNCTION update_chunk_search_vector()
RETURNS TRIGGER AS $$
BEGIN
    NEW.search_vector := to_tsvector('english', COALESCE(NEW.text, ''));
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

-- Create trigger to auto-update search_vector
DROP TRIGGER IF EXISTS chunk_search_vector_trigger ON content_chunks;
CREATE TRIGGER chunk_search_vector_trigger
    BEFORE INSERT OR UPDATE OF text ON content_chunks
    FOR EACH ROW
    EXECUTE FUNCTION update_chunk_search_vector();

-- Backfill existing chunks
UPDATE content_chunks SET search_vector = to_tsvector('english', COALESCE(text, ''))
WHERE search_vector IS NULL;

-- Add similar for content_items (for title/metadata search)
ALTER TABLE content_items ADD COLUMN IF NOT EXISTS search_vector tsvector;
CREATE INDEX IF NOT EXISTS idx_content_items_search ON content_items USING GIN(search_vector);

-- Create function to update content_items search_vector
-- Weight title higher ('A') than content ('B') for better relevance
CREATE OR REPLACE FUNCTION update_item_search_vector()
RETURNS TRIGGER AS $$
BEGIN
    NEW.search_vector := setweight(to_tsvector('english', COALESCE(NEW.title, '')), 'A') ||
                         setweight(to_tsvector('english', COALESCE(NEW.content, '')), 'B');
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS item_search_vector_trigger ON content_items;
CREATE TRIGGER item_search_vector_trigger
    BEFORE INSERT OR UPDATE OF title, content ON content_items
    FOR EACH ROW
    EXECUTE FUNCTION update_item_search_vector();

-- Backfill existing content_items
UPDATE content_items
SET search_vector = setweight(to_tsvector('english', COALESCE(title, '')), 'A') ||
                    setweight(to_tsvector('english', COALESCE(content, '')), 'B')
WHERE search_vector IS NULL;
