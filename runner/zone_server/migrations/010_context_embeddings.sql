-- Context Embeddings and Heuristic Analysis
-- Provides vector storage for semantic search and content analysis

-- Enable pgvector extension for vector similarity search
CREATE EXTENSION IF NOT EXISTS vector;

-- Content items: Normalized content from sources
CREATE TABLE content_items (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    source_id UUID NOT NULL REFERENCES sources(id) ON DELETE CASCADE,
    category TEXT NOT NULL,
    uri TEXT NOT NULL,
    title TEXT NOT NULL,
    content TEXT,  -- NULL if metadata_only
    content_type TEXT NOT NULL DEFAULT 'text/plain',
    token_count INTEGER NOT NULL DEFAULT 0,
    metadata_only BOOLEAN NOT NULL DEFAULT FALSE,
    content_hash TEXT NOT NULL,  -- SHA256 for deduplication
    metadata JSONB NOT NULL DEFAULT '{}'::jsonb,
    modified_at TIMESTAMP,
    fetched_at TIMESTAMP NOT NULL DEFAULT NOW(),
    created_at TIMESTAMP DEFAULT NOW(),
    updated_at TIMESTAMP DEFAULT NOW(),

    -- Prevent duplicate URIs per source
    UNIQUE(source_id, uri)
);

CREATE INDEX idx_content_items_source ON content_items(source_id);
CREATE INDEX idx_content_items_category ON content_items(category);
CREATE INDEX idx_content_items_hash ON content_items(content_hash);
CREATE INDEX idx_content_items_fetched ON content_items(fetched_at);
CREATE INDEX idx_content_items_metadata ON content_items USING GIN(metadata);

-- Content chunks: Split content for embedding
CREATE TABLE content_chunks (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    content_item_id UUID NOT NULL REFERENCES content_items(id) ON DELETE CASCADE,
    chunk_index INTEGER NOT NULL,
    text TEXT NOT NULL,
    token_count INTEGER NOT NULL,
    start_offset INTEGER NOT NULL,
    end_offset INTEGER NOT NULL,
    created_at TIMESTAMP DEFAULT NOW(),

    UNIQUE(content_item_id, chunk_index)
);

CREATE INDEX idx_content_chunks_item ON content_chunks(content_item_id);

-- Embeddings: Vector storage using pgvector
-- Using 1536 dimensions (OpenAI text-embedding-3-small compatible)
-- Can also work with smaller dimensions via padding/truncation
CREATE TABLE embeddings (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    chunk_id UUID NOT NULL REFERENCES content_chunks(id) ON DELETE CASCADE UNIQUE,
    content_item_id UUID NOT NULL REFERENCES content_items(id) ON DELETE CASCADE,
    source_id UUID NOT NULL REFERENCES sources(id) ON DELETE CASCADE,
    vector vector(1536) NOT NULL,
    model TEXT NOT NULL,
    created_at TIMESTAMP DEFAULT NOW()
);

-- HNSW index for fast approximate nearest neighbor search
-- Using cosine similarity (vector_cosine_ops)
CREATE INDEX idx_embeddings_vector ON embeddings
    USING hnsw (vector vector_cosine_ops)
    WITH (m = 16, ef_construction = 64);

CREATE INDEX idx_embeddings_source ON embeddings(source_id);
CREATE INDEX idx_embeddings_content_item ON embeddings(content_item_id);

-- Heuristic analysis results
CREATE TABLE heuristic_analysis (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    content_item_id UUID NOT NULL REFERENCES content_items(id) ON DELETE CASCADE UNIQUE,

    -- Extracted entities (people, dates, code refs, URLs, etc.)
    entities JSONB NOT NULL DEFAULT '{}'::jsonb,

    -- Content categorization (topic, sentiment, priority, actionability)
    categorization JSONB NOT NULL DEFAULT '{}'::jsonb,

    -- Quality metrics (freshness, reliability, density, duplication)
    quality JSONB NOT NULL DEFAULT '{}'::jsonb,

    analyzed_at TIMESTAMP NOT NULL DEFAULT NOW(),
    created_at TIMESTAMP DEFAULT NOW(),
    updated_at TIMESTAMP DEFAULT NOW()
);

CREATE INDEX idx_heuristic_analysis_item ON heuristic_analysis(content_item_id);
CREATE INDEX idx_heuristic_analysis_entities ON heuristic_analysis USING GIN(entities);
CREATE INDEX idx_heuristic_analysis_categorization ON heuristic_analysis USING GIN(categorization);

-- Chat message embeddings: For conversation history search
CREATE TABLE message_embeddings (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    message_id UUID NOT NULL REFERENCES messages(id) ON DELETE CASCADE UNIQUE,
    chat_id UUID NOT NULL REFERENCES chats(id) ON DELETE CASCADE,
    vector vector(1536) NOT NULL,
    model TEXT NOT NULL,
    created_at TIMESTAMP DEFAULT NOW()
);

CREATE INDEX idx_message_embeddings_vector ON message_embeddings
    USING hnsw (vector vector_cosine_ops)
    WITH (m = 16, ef_construction = 64);

CREATE INDEX idx_message_embeddings_chat ON message_embeddings(chat_id);

-- Knowledge base entries: User-curated content
CREATE TABLE knowledge_entries (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    workspace_id UUID NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    title TEXT NOT NULL,
    content TEXT NOT NULL,
    category TEXT,
    tags TEXT[] DEFAULT '{}',
    token_count INTEGER NOT NULL DEFAULT 0,
    is_active BOOLEAN DEFAULT TRUE,
    created_by UUID REFERENCES users(id) ON DELETE SET NULL,
    created_at TIMESTAMP DEFAULT NOW(),
    updated_at TIMESTAMP DEFAULT NOW()
);

CREATE INDEX idx_knowledge_entries_workspace ON knowledge_entries(workspace_id);
CREATE INDEX idx_knowledge_entries_active ON knowledge_entries(workspace_id, is_active);
CREATE INDEX idx_knowledge_entries_tags ON knowledge_entries USING GIN(tags);

-- Knowledge entry embeddings
CREATE TABLE knowledge_embeddings (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    knowledge_entry_id UUID NOT NULL REFERENCES knowledge_entries(id) ON DELETE CASCADE UNIQUE,
    workspace_id UUID NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    vector vector(1536) NOT NULL,
    model TEXT NOT NULL,
    created_at TIMESTAMP DEFAULT NOW()
);

CREATE INDEX idx_knowledge_embeddings_vector ON knowledge_embeddings
    USING hnsw (vector vector_cosine_ops)
    WITH (m = 16, ef_construction = 64);

CREATE INDEX idx_knowledge_embeddings_workspace ON knowledge_embeddings(workspace_id);

-- Context gathering runs: Job tracking for audit/history
CREATE TABLE context_gatherings (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    workspace_id UUID REFERENCES workspaces(id) ON DELETE SET NULL,
    task_id UUID REFERENCES tasks(id) ON DELETE SET NULL,
    status TEXT NOT NULL DEFAULT 'pending'
        CHECK (status IN ('pending', 'running', 'completed', 'failed')),
    source_ids UUID[] NOT NULL DEFAULT '{}',
    config JSONB NOT NULL DEFAULT '{}'::jsonb,
    stats JSONB,
    error_message TEXT,
    started_at TIMESTAMP,
    completed_at TIMESTAMP,
    created_at TIMESTAMP DEFAULT NOW()
);

CREATE INDEX idx_context_gatherings_workspace ON context_gatherings(workspace_id);
CREATE INDEX idx_context_gatherings_task ON context_gatherings(task_id);
CREATE INDEX idx_context_gatherings_status ON context_gatherings(status);

-- Source sync state: Track incremental sync progress
CREATE TABLE source_sync_state (
    source_id UUID PRIMARY KEY REFERENCES sources(id) ON DELETE CASCADE,
    last_sync_at TIMESTAMP,
    cursor TEXT,  -- Source-specific continuation token
    etag TEXT,    -- For HTTP-based sources
    version TEXT, -- For versioned sources
    extra JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMP DEFAULT NOW(),
    updated_at TIMESTAMP DEFAULT NOW()
);

CREATE INDEX idx_sync_state_last_sync ON source_sync_state(last_sync_at);

-- Function to search content embeddings by similarity
CREATE OR REPLACE FUNCTION search_content_embeddings(
    query_vector vector(1536),
    p_limit INTEGER DEFAULT 10,
    p_threshold FLOAT DEFAULT 0.7,
    p_source_ids UUID[] DEFAULT NULL,
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
        AND (p_categories IS NULL OR ci.category = ANY(p_categories))
    ORDER BY e.vector <=> query_vector
    LIMIT p_limit;
END;
$$ LANGUAGE plpgsql;

-- Function to search knowledge base
CREATE OR REPLACE FUNCTION search_knowledge(
    query_vector vector(1536),
    p_workspace_id UUID,
    p_limit INTEGER DEFAULT 10,
    p_threshold FLOAT DEFAULT 0.7
)
RETURNS TABLE(
    entry_id UUID,
    similarity FLOAT,
    title TEXT,
    content TEXT,
    category TEXT,
    tags TEXT[]
) AS $$
BEGIN
    RETURN QUERY
    SELECT
        ke.id as entry_id,
        (1 - (ke_embed.vector <=> query_vector))::FLOAT as similarity,
        ke.title,
        ke.content,
        ke.category,
        ke.tags
    FROM knowledge_entries ke
    JOIN knowledge_embeddings ke_embed ON ke_embed.knowledge_entry_id = ke.id
    WHERE ke.workspace_id = p_workspace_id
        AND ke.is_active = TRUE
        AND (1 - (ke_embed.vector <=> query_vector)) >= p_threshold
    ORDER BY ke_embed.vector <=> query_vector
    LIMIT p_limit;
END;
$$ LANGUAGE plpgsql;

-- Function to search chat message history
CREATE OR REPLACE FUNCTION search_chat_history(
    query_vector vector(1536),
    p_chat_id UUID,
    p_limit INTEGER DEFAULT 10,
    p_threshold FLOAT DEFAULT 0.7
)
RETURNS TABLE(
    message_id UUID,
    similarity FLOAT,
    role TEXT,
    content TEXT,
    created_at TIMESTAMP
) AS $$
BEGIN
    RETURN QUERY
    SELECT
        m.id as message_id,
        (1 - (me.vector <=> query_vector))::FLOAT as similarity,
        m.role,
        m.content,
        m.created_at
    FROM message_embeddings me
    JOIN messages m ON m.id = me.message_id
    WHERE me.chat_id = p_chat_id
        AND (1 - (me.vector <=> query_vector)) >= p_threshold
    ORDER BY me.vector <=> query_vector
    LIMIT p_limit;
END;
$$ LANGUAGE plpgsql;
