-- Get a single message by ID
SELECT id, chat_id, role, content, created_at::timestamp FROM messages
WHERE id = $1
