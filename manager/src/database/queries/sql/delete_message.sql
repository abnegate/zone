-- Delete a message by ID
DELETE FROM messages
WHERE id = $1
