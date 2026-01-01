-- Get queued tasks for display/monitoring
SELECT t.id, t.project_id, t.title, t.description, t.acceptance_criteria, t.status,
       t.priority, t.model_name, t.dependencies, t.created_at, t.updated_at,
       t.started_at, t.completed_at, t.is_agentic, t.github_repo_url, t.queued_at, t.worker_id
FROM tasks t
JOIN task_queue tq ON tq.task_id = t.id
ORDER BY tq.priority DESC, tq.queued_at ASC
