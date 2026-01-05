-- List only active organizations ordered by name
SELECT id, name, slug, description, is_active, created_at::timestamp, updated_at::timestamp FROM organizations
WHERE is_active = true
ORDER BY name ASC
