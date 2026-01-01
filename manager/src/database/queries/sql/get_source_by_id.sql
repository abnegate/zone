-- Get a single source by ID
SELECT id, name, source_type, config, credentials_encrypted, description, url,
       is_active, last_verified_at, last_error, created_at, updated_at
FROM sources
WHERE id = $1
