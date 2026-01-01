-- Add a log entry to a task run
INSERT INTO task_run_logs (task_run_id, phase, agent_type, log_level, message, created_at)
VALUES ($1, $2, $3, $4, $5, $6)
RETURNING id, task_run_id, phase, agent_type, log_level, message, created_at
