-- 'unlink' records an external issue deleted, which leaves its task in place
-- but no longer linked to it. The inline CHECK from the initial schema was
-- auto-named sync_events_event_type_check by Postgres. It is added NOT VALID so
-- sync_events is not scanned under this ACCESS EXCLUSIVE lock; 052 proves it.
--
-- sync_deliveries holds the ID GitHub (X-GitHub-Delivery) or Linear
-- (Linear-Delivery) gave each webhook delivery a sync applied, so a provider's
-- retry of a delivery it already made is skipped. The ID is not signed, so it
-- cannot stop a body replayed under a new or absent ID.
SET LOCAL lock_timeout = '5s';
ALTER TABLE sync_events DROP CONSTRAINT IF EXISTS sync_events_event_type_check;
ALTER TABLE sync_events ADD CONSTRAINT sync_events_event_type_check
    CHECK (event_type IN ('create', 'update', 'close', 'unlink', 'webhook_received', 'sync_error')) NOT VALID;

CREATE TABLE sync_deliveries (
    sync_config_id UUID NOT NULL REFERENCES sync_configs(id) ON DELETE CASCADE,
    delivery_id TEXT NOT NULL,
    received_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (sync_config_id, delivery_id)
);
