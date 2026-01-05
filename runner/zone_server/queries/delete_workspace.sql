-- Delete a workspace by ID
DELETE FROM workspaces
WHERE id = $1 AND organization_id = $2
