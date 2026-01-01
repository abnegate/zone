-- Update an existing workspace
UPDATE workspaces
SET name = $1, slug = $2, description = $3, is_active = $4, updated_at = $5
WHERE id = $6 AND organization_id = $7
RETURNING id, organization_id, name, slug, description, is_active, created_at::timestamp, updated_at::timestamp