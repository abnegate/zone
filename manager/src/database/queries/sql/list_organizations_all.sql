-- List all organizations ordered by name
SELECT id, name, slug, description, is_active, created_at, updated_at
FROM organizations
ORDER BY name ASC
