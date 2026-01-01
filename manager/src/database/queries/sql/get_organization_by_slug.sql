-- Get a single organization by slug
SELECT id, name, slug, description, is_active, created_at, updated_at
FROM organizations
WHERE slug = $1
