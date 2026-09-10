-- The chat is the security boundary for a citation: the model may only cite what
-- this conversation retrieved, so the registry that proves a citation is scoped to
-- one chat. A workspace-wide registry would resolve an identifier against a source
-- surfaced in someone else's chat and let the model cite it.
CREATE TABLE chat_sources (
    chat_id UUID NOT NULL REFERENCES chats(id) ON DELETE CASCADE,
    identifier TEXT NOT NULL,
    kind TEXT NOT NULL CHECK (kind IN ('web', 'doc', 'kb', 'chat')),
    uri TEXT NOT NULL,
    title TEXT NOT NULL,
    first_observed_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    last_observed_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    PRIMARY KEY (chat_id, identifier),
    UNIQUE (chat_id, kind, uri)
);
