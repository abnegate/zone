-- An external issue whose deletion unlinked it from its task. Delivery IDs and
-- GitHub's event header are not signed, so a replayed `opened` body can arrive
-- under a fresh ID; an issue recorded here never becomes a task again.
SET LOCAL lock_timeout = '5s';
CREATE TABLE sync_unlinked_items (
    sync_config_id UUID NOT NULL REFERENCES sync_configs(id) ON DELETE CASCADE,
    external_id TEXT NOT NULL,
    unlinked_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (sync_config_id, external_id)
);
