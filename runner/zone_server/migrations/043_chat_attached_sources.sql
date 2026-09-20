-- The sources a chat is pinned to. Distinct from chat_sources, which is the
-- registry of what a chat has already retrieved and may cite: this table is
-- chosen by the person before a turn, and narrows what that turn retrieves.
-- A source belongs to the chat's workspace; the write path checks that, since
-- a foreign key cannot say so across the two tables.
CREATE TABLE chat_attached_sources (
    chat_id UUID NOT NULL REFERENCES chats(id) ON DELETE CASCADE,
    source_id UUID NOT NULL REFERENCES sources(id) ON DELETE CASCADE,
    attached_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (chat_id, source_id)
);

CREATE INDEX idx_chat_attached_sources_source ON chat_attached_sources(source_id);
