-- Create a new organization
INSERT INTO organizations (name, slug, description, created_at, updated_at)
VALUES ($1, $2, $3, $4, $5)
RETURNING id, name, slug, description, is_active, created_at::timestamp, updated_at::timestamp