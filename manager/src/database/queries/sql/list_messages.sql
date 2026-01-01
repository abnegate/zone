-- List messages for a chat ordered by created_at
SELECT id, chat_id, role, content, created_at
FROM messages
WHERE chat_id = $1
ORDER BY created_at ASC
