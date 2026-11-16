SET LOCAL lock_timeout = '5s';

UPDATE sources SET is_active = false, updated_at = NOW()
WHERE source_type = 'notion' AND is_active IS DISTINCT FROM false;
