-- The chat is the security boundary for a citation: the model may only cite what
-- this conversation retrieved, so the registry that proves a citation is scoped to
-- one chat. A workspace-wide registry would resolve an identifier against a source
-- surfaced in someone else's chat and let the model cite it.
--
-- A source is identified by `key` and addressed by `uri`, and they are not always
-- the same string. A knowledge passage is keyed by the entry it came from and
-- addressed by the URL a reader would open. Hashing the key keeps the identifier
-- stable across turns; storing the address separately keeps a citation resolved
-- from this table equal to the one the retrieval envelope already carries, so the
-- two deduplicate into a single citation instead of doubling up.
CREATE TABLE chat_sources (
    chat_id UUID NOT NULL REFERENCES chats(id) ON DELETE CASCADE,
    identifier TEXT NOT NULL,
    kind TEXT NOT NULL CHECK (kind IN ('web', 'doc', 'kb', 'chat')),
    key TEXT NOT NULL,
    uri TEXT NOT NULL,
    title TEXT NOT NULL,
    first_observed_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    last_observed_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    PRIMARY KEY (chat_id, identifier),
    CONSTRAINT chat_sources_identity UNIQUE (chat_id, kind, key)
);
