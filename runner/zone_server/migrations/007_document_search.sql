-- Workspace notes and stored source documents remain searchable without an embedding service.
CREATE INDEX IF NOT EXISTS idx_knowledge_documents_search
    ON knowledge_entries USING GIN (to_tsvector('english', title || ' ' || COALESCE(content, '')))
    WHERE is_active = TRUE;

CREATE INDEX IF NOT EXISTS idx_content_documents_search
    ON content_items USING GIN (to_tsvector('english', title || ' ' || COALESCE(CASE WHEN metadata_only THEN NULL ELSE content END, '')));
