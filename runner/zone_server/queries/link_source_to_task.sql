-- Link a source to a task
UPDATE tasks
SET source_id = $1, updated_at = $2
WHERE id = $3
