-- List active sources only
SELECT id, name, source_type, config, credentials_encrypted, description, url,
       is_active, last_verified_at::timestamp, last_error, created_at::timestamp, updated_at::timestamp
FROM sources
WHERE is_active = TRUE
ORDER BY name ASC
