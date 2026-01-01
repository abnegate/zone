-- List all organizations ordered by name
SELECT id, name, slug, description, is_active, created_at::timestamp, updated_at::timestamp FROM organizations
ORDER BY name ASC
