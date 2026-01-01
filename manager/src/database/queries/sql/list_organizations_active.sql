-- List only active organizations ordered by name
SELECT id, name, slug, description, is_active, created_at, updated_at
FROM organizations
WHERE is_active = true
ORDER BY name ASC
