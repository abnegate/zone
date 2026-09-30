SET LOCAL lock_timeout = '5s';
ALTER TABLE sync_events VALIDATE CONSTRAINT sync_events_event_type_check;
