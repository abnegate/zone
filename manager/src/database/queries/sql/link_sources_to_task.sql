-- Link multiple sources to a task
UPDATE tasks
SET source_ids = $1::uuid[], updated_at = $2
WHERE id = $3
