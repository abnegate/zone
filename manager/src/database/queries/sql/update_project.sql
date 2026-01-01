-- Update an existing project
UPDATE projects
SET name = $1, description = $2, status = $3, github_repo_url = $4, updated_at = $5
WHERE id = $6
RETURNING id, name, description, status, github_repo_url, created_at, updated_at
