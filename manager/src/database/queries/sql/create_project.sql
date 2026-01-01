-- Create a new project
INSERT INTO projects (name, description, status, github_repo_url, created_at, updated_at)
VALUES ($1, $2, $3, $4, $5, $6)
RETURNING id, name, description, status, github_repo_url, created_at, updated_at
