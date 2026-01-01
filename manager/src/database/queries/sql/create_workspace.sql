-- Create a new workspace within an organization
INSERT INTO workspaces (organization_id, name, slug, description, created_at, updated_at)
VALUES ($1, $2, $3, $4, $5, $6)
RETURNING id, organization_id, name, slug, description, is_active, created_at, updated_at
