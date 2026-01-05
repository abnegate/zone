-- Update user active status
UPDATE users
SET is_active = $1, updated_at = NOW()
WHERE id = $2
