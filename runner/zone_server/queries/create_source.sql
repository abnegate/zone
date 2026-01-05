-- Create a new source
INSERT INTO sources (name, source_type, config, credentials_encrypted, description, url, created_at, updated_at)
VALUES ($1, $2, $3::jsonb, $4, $5, $6, $7, $8)
RETURNING id, name, source_type, config, credentials_encrypted, description, url,
          is_active, last_verified_at::timestamp, last_error, created_at::timestamp, updated_at::timestamp