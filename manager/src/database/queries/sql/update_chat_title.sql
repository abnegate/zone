-- Update chat title
UPDATE chats
SET title = $1, updated_at = $2
WHERE id = $3
RETURNING id, title, model_name, created_at::timestamp, updated_at::timestamp, archived
