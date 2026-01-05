-- Link a source to a project
UPDATE projects
SET source_id = $1, updated_at = $2
WHERE id = $3
