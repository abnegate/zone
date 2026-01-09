-- External Issue Tracker Sync Configuration
-- Migration 021

BEGIN;

-- Sync configurations per project
-- Stores settings for syncing tasks with external issue trackers like GitHub and Linear
CREATE TABLE sync_configs (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    provider TEXT NOT NULL CHECK (provider IN ('github', 'linear')),
    enabled BOOLEAN NOT NULL DEFAULT true,
    config JSONB NOT NULL DEFAULT '{}'::jsonb,
    -- Encrypted webhook secret for verifying incoming webhooks
    -- Format: base64(nonce || ciphertext) as returned by crypto::encrypt
    webhook_secret_encrypted TEXT,
    created_at TIMESTAMP DEFAULT NOW(),
    updated_at TIMESTAMP DEFAULT NOW(),
    -- Only one config per provider per project
    UNIQUE(project_id, provider)
);

CREATE INDEX idx_sync_configs_project_id ON sync_configs(project_id);
CREATE INDEX idx_sync_configs_provider ON sync_configs(provider);
CREATE INDEX idx_sync_configs_enabled ON sync_configs(enabled) WHERE enabled = true;

-- Track synced items (task <-> external issue mappings)
CREATE TABLE synced_items (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    sync_config_id UUID NOT NULL REFERENCES sync_configs(id) ON DELETE CASCADE,
    task_id UUID NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    -- External identifier (GitHub issue number, Linear issue ID)
    external_id TEXT NOT NULL,
    -- URL to the external issue
    external_url TEXT,
    last_synced_at TIMESTAMP DEFAULT NOW(),
    -- Direction of sync: 'outbound' (Zone -> External), 'inbound' (External -> Zone), 'bidirectional'
    sync_direction TEXT NOT NULL DEFAULT 'bidirectional' CHECK (sync_direction IN ('outbound', 'inbound', 'bidirectional')),
    -- Store last known state from external system for conflict detection
    last_external_state JSONB,
    created_at TIMESTAMP DEFAULT NOW(),
    -- Only one sync per task per config
    UNIQUE(sync_config_id, task_id),
    -- Only one task per external ID per config
    UNIQUE(sync_config_id, external_id)
);

CREATE INDEX idx_synced_items_sync_config_id ON synced_items(sync_config_id);
CREATE INDEX idx_synced_items_task_id ON synced_items(task_id);
CREATE INDEX idx_synced_items_external_id ON synced_items(sync_config_id, external_id);
CREATE INDEX idx_synced_items_last_synced ON synced_items(last_synced_at);

-- Sync event log for debugging and audit
CREATE TABLE sync_events (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    sync_config_id UUID NOT NULL REFERENCES sync_configs(id) ON DELETE CASCADE,
    synced_item_id UUID REFERENCES synced_items(id) ON DELETE SET NULL,
    event_type TEXT NOT NULL CHECK (event_type IN ('create', 'update', 'close', 'webhook_received', 'sync_error')),
    direction TEXT NOT NULL CHECK (direction IN ('outbound', 'inbound')),
    -- Event payload (webhook data, API response, error details)
    payload JSONB,
    error_message TEXT,
    created_at TIMESTAMP DEFAULT NOW()
);

CREATE INDEX idx_sync_events_sync_config_id ON sync_events(sync_config_id);
CREATE INDEX idx_sync_events_synced_item_id ON sync_events(synced_item_id);
CREATE INDEX idx_sync_events_event_type ON sync_events(event_type);
CREATE INDEX idx_sync_events_created_at ON sync_events(created_at DESC);

-- Update trigger for sync_configs
CREATE OR REPLACE FUNCTION update_sync_configs_updated_at()
RETURNS TRIGGER AS $$
BEGIN
    NEW.updated_at = NOW();
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER sync_configs_updated_at
    BEFORE UPDATE ON sync_configs
    FOR EACH ROW
    EXECUTE FUNCTION update_sync_configs_updated_at();

-- Record migration
INSERT INTO schema_migrations (version) VALUES (21);

COMMIT;
