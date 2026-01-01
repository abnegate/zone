-- List all projects ordered by updated_at
SELECT id, name, description, status, github_repo_url, created_at, updated_at
FROM projects
ORDER BY updated_at DESC
