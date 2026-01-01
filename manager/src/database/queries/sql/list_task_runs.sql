-- List runs for a task ordered by started_at descending
SELECT id, task_id, status, current_phase, progress_percent,
       started_at::timestamp, completed_at::timestamp, error_message
FROM task_runs
WHERE task_id = $1
ORDER BY started_at DESC
