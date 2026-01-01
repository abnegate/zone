-- List only active workspaces for an organization ordered by name
SELECT id, organization_id, name, slug, description, is_active, created_at::timestamp, updated_at::timestamp FROM workspaces
WHERE organization_id = $1 AND is_active = true
ORDER BY name ASC
