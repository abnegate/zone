-- List all tasks ordered by priority and updated_at::timestamp
SELECT id, project_id, title, description, acceptance_criteria, status,
       priority, model_name, dependencies, created_at::timestamp, updated_at::timestamp, started_at::timestamp, completed_at::timestamp, is_agentic, github_repo_url, queued_at::timestamp, worker_id
FROM tasks
ORDER BY priority ASC, updated_at DESC
