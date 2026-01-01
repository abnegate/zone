-- List archived chats ordered by updated_at
SELECT id, title, model_name, created_at, updated_at, archived
FROM chats
WHERE archived = true
ORDER BY updated_at DESC
