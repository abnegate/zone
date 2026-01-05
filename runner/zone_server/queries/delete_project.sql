-- Delete a project by ID
DELETE FROM projects
WHERE id = $1
