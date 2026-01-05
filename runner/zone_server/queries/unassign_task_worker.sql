-- Clear worker assignment from task
UPDATE tasks
SET worker_id = NULL, updated_at = $1
WHERE id = $2
RETURNING id, project_id, title, description, acceptance_criteria, status,
          priority, model_name, dependencies, created_at::timestamp, updated_at::timestamp, started_at::timestamp, completed_at::timestamp, is_agentic, github_repo_url, queued_at::timestamp, worker_id
