-- Revoke a refresh token
UPDATE refresh_tokens
SET revoked_at = NOW()
WHERE token_hash = $1 AND revoked_at IS NULL
