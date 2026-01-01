-- Get a single project by ID
SELECT id, name, description, status, github_repo_url, created_at, updated_at
FROM projects
WHERE id = $1
