-- Get a single organization by slug
SELECT id, name, slug, description, is_active, created_at::timestamp, updated_at::timestamp FROM organizations
WHERE slug = $1
