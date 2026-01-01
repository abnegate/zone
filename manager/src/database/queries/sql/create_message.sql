-- Create a new message
INSERT INTO messages (chat_id, role, content, created_at)
VALUES ($1, $2, $3, $4)
RETURNING id, chat_id, role, content, created_at
