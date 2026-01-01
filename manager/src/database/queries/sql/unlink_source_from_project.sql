-- Unlink source from a project
UPDATE projects
SET source_id = NULL, updated_at = $1
WHERE id = $2
