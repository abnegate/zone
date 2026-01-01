-- List sources by type (all)
SELECT id, name, source_type, config, credentials_encrypted, description, url,
       is_active, last_verified_at::timestamp, last_error, created_at::timestamp, updated_at::timestamp
FROM sources
WHERE source_type = $1
ORDER BY name ASC
