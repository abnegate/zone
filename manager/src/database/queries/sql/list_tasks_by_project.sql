-- List tasks for a specific project ordered by priority and updated_at
SELECT id, project_id, title, description, acceptance_criteria, status,
       priority, model_name, dependencies, created_at, updated_at,
       started_at, completed_at, is_agentic, github_repo_url, queued_at, worker_id
FROM tasks
WHERE project_id = $1
ORDER BY priority ASC, updated_at DESC
