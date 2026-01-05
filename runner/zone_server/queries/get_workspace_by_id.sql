-- Get a single workspace by ID (verify it belongs to the organization)
SELECT id, organization_id, name, slug, description, is_active, created_at::timestamp, updated_at::timestamp FROM workspaces
WHERE id = $1 AND organization_id = $2
