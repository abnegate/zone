-- Get a single chat by ID
SELECT id, title, model_name, created_at::timestamp, updated_at::timestamp, archived
FROM chats
WHERE id = $1
