-- List all workspaces for an organization ordered by name
SELECT id, organization_id, name, slug, description, is_active, created_at::timestamp, updated_at::timestamp FROM workspaces
WHERE organization_id = $1
ORDER BY name ASC
