-- Canonical replay survives interrupted generations independently of visible assistant rows.
CREATE TABLE chat_leases (
    chat_id UUID PRIMARY KEY REFERENCES chats(id) ON DELETE CASCADE,
    owner UUID NOT NULL,
    fence BIGINT NOT NULL CHECK (fence > 0),
    expires_at TIMESTAMPTZ NOT NULL
);

CREATE TABLE chat_turns (
    id UUID PRIMARY KEY,
    chat_id UUID NOT NULL REFERENCES chats(id) ON DELETE CASCADE,
    user_message_id UUID NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
    fence BIGINT NOT NULL,
    version SMALLINT NOT NULL DEFAULT 1 CHECK (version = 1),
    status TEXT NOT NULL DEFAULT 'running' CHECK (status IN ('running', 'completed', 'interrupted')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    completed_at TIMESTAMPTZ,
    UNIQUE (chat_id, id),
    UNIQUE (chat_id, user_message_id)
);
CREATE INDEX chat_turns_running ON chat_turns(chat_id) WHERE status = 'running';

CREATE TABLE chat_entries (
    chat_id UUID NOT NULL REFERENCES chats(id) ON DELETE CASCADE,
    id TEXT NOT NULL,
    position BIGINT GENERATED ALWAYS AS IDENTITY,
    turn_id UUID,
    message JSONB NOT NULL,
    consumed BOOLEAN NOT NULL DEFAULT FALSE,
    legacy BOOLEAN NOT NULL DEFAULT FALSE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    PRIMARY KEY (chat_id, id),
    UNIQUE (chat_id, position),
    FOREIGN KEY (chat_id, turn_id) REFERENCES chat_turns(chat_id, id) ON DELETE CASCADE,
    CHECK (message->>'version' = '1'),
    CHECK (message->>'role' IN ('system', 'user', 'assistant', 'tool'))
);

CREATE TABLE chat_calls (
    chat_id UUID NOT NULL,
    id TEXT NOT NULL,
    turn_id UUID NOT NULL,
    envelope_id TEXT NOT NULL,
    result_id TEXT,
    mutating BOOLEAN NOT NULL,
    PRIMARY KEY (chat_id, id),
    FOREIGN KEY (chat_id, turn_id) REFERENCES chat_turns(chat_id, id) ON DELETE CASCADE,
    FOREIGN KEY (chat_id, envelope_id) REFERENCES chat_entries(chat_id, id) ON DELETE CASCADE,
    FOREIGN KEY (chat_id, result_id) REFERENCES chat_entries(chat_id, id) ON DELETE CASCADE,
    UNIQUE (chat_id, result_id)
);
CREATE INDEX chat_calls_pending ON chat_calls(chat_id, turn_id) WHERE result_id IS NULL;

CREATE TABLE chat_checkpoints (
    chat_id UUID PRIMARY KEY REFERENCES chats(id) ON DELETE CASCADE,
    revision BIGINT NOT NULL CHECK (revision > 0),
    content TEXT NOT NULL CHECK (length(trim(content)) > 0),
    entries JSONB NOT NULL CHECK (jsonb_typeof(entries) = 'array'),
    fingerprint TEXT NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp()
);
