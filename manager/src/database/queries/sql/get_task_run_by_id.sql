-- Get a task run by ID
SELECT id, task_id, status, current_phase, progress_percent,
       started_at, completed_at, error_message
FROM task_runs
WHERE id = $1
