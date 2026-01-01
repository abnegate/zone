-- Unlink GitHub repository from a project
UPDATE projects
SET github_repo_url = NULL, updated_at = $1
WHERE id = $2
RETURNING id, name, description, status, github_repo_url, created_at, updated_at
