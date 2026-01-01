-- List archived chats ordered by updated_at::timestamp
SELECT id, title, model_name, created_at::timestamp, updated_at::timestamp, archived
FROM chats
WHERE archived = true
ORDER BY updated_at DESC
