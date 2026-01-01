-- Get count of active tokens for a user
SELECT COUNT(*)::int
FROM refresh_tokens
WHERE user_id = $1 AND expires_at > NOW() AND revoked_at IS NULL
