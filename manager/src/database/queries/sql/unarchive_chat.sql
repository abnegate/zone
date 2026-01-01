-- Unarchive a chat
UPDATE chats
SET archived = false, updated_at = $1
WHERE id = $2
RETURNING id, title, model_name, created_at, updated_at, archived
