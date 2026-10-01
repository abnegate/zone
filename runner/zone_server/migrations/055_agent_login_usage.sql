SET LOCAL lock_timeout = '5s';

ALTER TABLE agent_logins DROP CONSTRAINT IF EXISTS agent_logins_organization_id_agent_key;
ALTER TABLE agent_logins ADD COLUMN IF NOT EXISTS account TEXT;
ALTER TABLE agent_logins ADD COLUMN IF NOT EXISTS windows JSONB;
ALTER TABLE agent_logins ADD COLUMN IF NOT EXISTS headroom DOUBLE PRECISION;
ALTER TABLE agent_logins ADD COLUMN IF NOT EXISTS usage_fetched_at TIMESTAMPTZ;
ALTER TABLE agent_logins ADD COLUMN IF NOT EXISTS exhausted_until TIMESTAMPTZ;
ALTER TABLE agent_logins ADD COLUMN IF NOT EXISTS last_used_at TIMESTAMPTZ;
CREATE INDEX IF NOT EXISTS idx_agent_logins_organization_agent
    ON agent_logins(organization_id, agent);
CREATE UNIQUE INDEX IF NOT EXISTS agent_logins_organization_agent_account_key
    ON agent_logins(organization_id, agent, account) WHERE account IS NOT NULL;

ALTER TABLE chats ADD COLUMN IF NOT EXISTS agent_login_id UUID;
ALTER TABLE chats ADD COLUMN IF NOT EXISTS agent_session_id TEXT;
ALTER TABLE chats ADD COLUMN IF NOT EXISTS agent_session_agent TEXT;
ALTER TABLE chats ADD COLUMN IF NOT EXISTS agent_session_entry BIGINT;
ALTER TABLE chats ADD COLUMN IF NOT EXISTS agent_session_prompt TEXT;
ALTER TABLE chats DROP CONSTRAINT IF EXISTS chats_agent_login_id_fkey;
ALTER TABLE chats ADD CONSTRAINT chats_agent_login_id_fkey
    FOREIGN KEY (agent_login_id) REFERENCES agent_logins(id) ON DELETE SET NULL NOT VALID;
