-- Validate a refresh token and get user_id
SELECT user_id
FROM refresh_tokens
WHERE token_hash = $1
  AND expires_at > NOW()
  AND revoked_at IS NULL
