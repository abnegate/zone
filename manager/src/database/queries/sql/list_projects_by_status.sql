-- List projects filtered by status ordered by updated_at
SELECT id, name, description, status, github_repo_url, created_at, updated_at
FROM projects
WHERE status = $1
ORDER BY updated_at DESC
