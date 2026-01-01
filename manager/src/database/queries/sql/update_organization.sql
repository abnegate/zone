-- Update an existing organization
UPDATE organizations
SET name = $1, slug = $2, description = $3, is_active = $4, updated_at = $5
WHERE id = $6
RETURNING id, name, slug, description, is_active, created_at, updated_at
