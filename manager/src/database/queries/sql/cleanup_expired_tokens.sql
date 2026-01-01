-- Clean up expired tokens
DELETE FROM refresh_tokens
WHERE expires_at < NOW()
