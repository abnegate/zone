-- Update task status to complete
UPDATE tasks
SET status = $1, completed_at = $2, updated_at = $2
WHERE id = $3
RETURNING id, project_id, title, description, acceptance_criteria, status,
          priority, model_name, dependencies, created_at::timestamp, updated_at::timestamp, started_at::timestamp, completed_at::timestamp, is_agentic, github_repo_url, queued_at::timestamp, worker_id
