-- List all projects ordered by updated_at::timestamp
SELECT id, name, description, status, github_repo_url, created_at::timestamp, updated_at::timestamp FROM projects
ORDER BY updated_at DESC
