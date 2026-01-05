-- Store a refresh token (hashed)
INSERT INTO refresh_tokens (user_id, token_hash, expires_at, user_agent, ip_address)
VALUES ($1, $2, $3::timestamp, $4, $5)
