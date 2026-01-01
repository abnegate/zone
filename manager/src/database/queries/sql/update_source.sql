-- Update an existing source
UPDATE sources
SET name = $1, config = $2::jsonb, credentials_encrypted = $3,
    description = $4, url = $5, is_active = $6, updated_at = $7
WHERE id = $8
RETURNING id, name, source_type, config, credentials_encrypted, description, url,
          is_active, last_verified_at, last_error, created_at, updated_at
