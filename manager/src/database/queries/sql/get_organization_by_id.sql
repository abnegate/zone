-- Get a single organization by ID
SELECT id, name, slug, description, is_active, created_at, updated_at
FROM organizations
WHERE id = $1
