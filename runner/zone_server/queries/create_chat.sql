-- Create a new chat
INSERT INTO chats (title, model_name, created_at, updated_at, archived)
VALUES ($1, $2, $3, $4, false)
RETURNING id, title, model_name, created_at::timestamp, updated_at::timestamp, archived
