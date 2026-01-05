-- Update last login time
UPDATE users
SET last_login_at = NOW()
WHERE id = $1
