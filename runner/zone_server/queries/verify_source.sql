-- Verify a source connection and update status
UPDATE sources
SET last_verified_at = $1, last_error = $2, updated_at = $3
WHERE id = $4
