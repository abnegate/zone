-- Get a single organization by ID
SELECT id, name, slug, description, is_active, created_at::timestamp, updated_at::timestamp FROM organizations
WHERE id = $1
