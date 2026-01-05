-- List tasks filtered by project and status ordered by priority and updated_at::timestamp
SELECT id, project_id, title, description, acceptance_criteria, status,
       priority, model_name, dependencies, created_at::timestamp, updated_at::timestamp, started_at::timestamp, completed_at::timestamp, is_agentic, github_repo_url, queued_at::timestamp, worker_id
FROM tasks
WHERE project_id = $1 AND status = $2
ORDER BY priority ASC, updated_at DESC
