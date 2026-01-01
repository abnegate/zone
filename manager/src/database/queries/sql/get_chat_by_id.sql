-- Get a single chat by ID
SELECT id, title, model_name, created_at, updated_at, archived
FROM chats
WHERE id = $1
