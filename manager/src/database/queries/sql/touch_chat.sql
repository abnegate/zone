-- Update chat's updated_at timestamp
UPDATE chats
SET updated_at = $1
WHERE id = $2
