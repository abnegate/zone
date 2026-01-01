-- List projects filtered by status ordered by updated_at::timestamp
SELECT id, name, description, status, github_repo_url, created_at::timestamp, updated_at::timestamp FROM projects
WHERE status = $1
ORDER BY updated_at DESC
