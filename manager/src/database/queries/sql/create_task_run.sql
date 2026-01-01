-- Create a new task run
INSERT INTO task_runs (task_id, status, progress_percent, started_at)
VALUES ($1, 'running', 0, $2)
RETURNING id, task_id, status, current_phase, progress_percent,
          started_at, completed_at, error_message
