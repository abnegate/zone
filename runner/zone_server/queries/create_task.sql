-- Create a new task
INSERT INTO tasks (project_id, title, description, acceptance_criteria,
                   status, priority, model_name, dependencies,
                   is_agentic, github_repo_url, created_at, updated_at)
VALUES ($1, $2, $3, $4, 'created', $5, $6, $7, $8, $9, $10, $11)
RETURNING id, project_id, title, description, acceptance_criteria, status,
          priority, model_name, dependencies, created_at::timestamp, updated_at::timestamp, started_at::timestamp, completed_at::timestamp, is_agentic, github_repo_url, queued_at::timestamp, worker_id
