-- List messages for a chat ordered by created_at::timestamp
SELECT id, chat_id, role, content, created_at::timestamp FROM messages
WHERE chat_id = $1
ORDER BY created_at ASC
