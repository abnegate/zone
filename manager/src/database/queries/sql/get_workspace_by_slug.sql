-- Get a single workspace by slug within an organization
SELECT id, organization_id, name, slug, description, is_active, created_at, updated_at
FROM workspaces
WHERE organization_id = $1 AND slug = $2
