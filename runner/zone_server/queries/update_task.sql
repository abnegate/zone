-- Update an existing task
UPDATE tasks
SET title = $1, description = $2, acceptance_criteria = $3,
    status = $4, priority = $5, model_name = $6, dependencies = $7,
    is_agentic = $8, github_repo_url = $9, updated_at = $10
WHERE id = $11
RETURNING id, project_id, title, description, acceptance_criteria, status,
          priority, model_name, dependencies, created_at::timestamp, updated_at::timestamp, started_at::timestamp, completed_at::timestamp, is_agentic, github_repo_url, queued_at::timestamp, worker_id
