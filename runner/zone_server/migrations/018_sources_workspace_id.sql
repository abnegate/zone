-- Add workspace_id to sources table for proper authorization
-- Migration 018

BEGIN;

-- Add workspace_id column to sources table
ALTER TABLE sources ADD COLUMN workspace_id UUID REFERENCES workspaces(id) ON DELETE CASCADE;

-- Create index for workspace lookups
CREATE INDEX idx_sources_workspace ON sources(workspace_id);

-- For existing sources, we need to handle the NULL workspace_id case
-- In a production system, you would need to migrate existing data appropriately
-- For now, we'll allow NULL and handle it in the application layer

-- Update content_items to include workspace_id for faster filtering
ALTER TABLE content_items ADD COLUMN workspace_id UUID REFERENCES workspaces(id) ON DELETE CASCADE;
CREATE INDEX idx_content_items_workspace ON content_items(workspace_id);

-- Update embeddings to include workspace_id for faster filtering
ALTER TABLE embeddings ADD COLUMN workspace_id UUID REFERENCES workspaces(id) ON DELETE CASCADE;
CREATE INDEX idx_embeddings_workspace_new ON embeddings(workspace_id);

-- Update the search_content_embeddings function to support workspace filtering
CREATE OR REPLACE FUNCTION search_content_embeddings(
    query_vector vector(1536),
    p_limit INTEGER DEFAULT 10,
    p_threshold FLOAT DEFAULT 0.7,
    p_source_ids UUID[] DEFAULT NULL,
    p_workspace_id UUID DEFAULT NULL,
    p_categories TEXT[] DEFAULT NULL
)
RETURNS TABLE(
    chunk_id UUID,
    content_item_id UUID,
    source_id UUID,
    similarity FLOAT,
    chunk_text TEXT,
    item_uri TEXT,
    item_title TEXT,
    item_metadata JSONB
) AS $$
BEGIN
    RETURN QUERY
    SELECT
        e.chunk_id,
        e.content_item_id,
        e.source_id,
        (1 - (e.vector <=> query_vector))::FLOAT as similarity,
        cc.text as chunk_text,
        ci.uri as item_uri,
        ci.title as item_title,
        ci.metadata as item_metadata
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

COMMIT;
