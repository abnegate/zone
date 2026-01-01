-- Link a GitHub repository to a project
UPDATE projects
SET github_repo_url = $1, updated_at = $2
WHERE id = $3
RETURNING id, name, description, status, github_repo_url, created_at, updated_at
