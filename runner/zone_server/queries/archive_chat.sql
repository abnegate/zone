-- Archive a chat
UPDATE chats
SET archived = true, updated_at = $1
WHERE id = $2
RETURNING id, title, model_name, created_at::timestamp, updated_at::timestamp, archived
