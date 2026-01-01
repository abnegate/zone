-- Get a single project by ID
SELECT id, name, description, status, github_repo_url, created_at::timestamp, updated_at::timestamp FROM projects
WHERE id = $1
