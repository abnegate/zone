-- Update task status to complete
UPDATE tasks
SET status = $1, completed_at = $2, updated_at = $2
WHERE id = $3
RETURNING id, project_id, title, description, acceptance_criteria, status,
          priority, model_name, dependencies, created_at, updated_at,
          started_at, completed_at, is_agentic, github_repo_url, queued_at, worker_id
