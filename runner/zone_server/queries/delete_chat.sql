-- Delete a chat (messages cascade delete)
DELETE FROM chats
WHERE id = $1
