-- List logs for a task run ordered by created_at
SELECT id, task_run_id, phase, agent_type, log_level, message, created_at
FROM task_run_logs
WHERE task_run_id = $1
ORDER BY created_at ASC
