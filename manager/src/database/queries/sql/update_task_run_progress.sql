-- Update task run progress
UPDATE task_runs
SET current_phase = $1, progress_percent = $2
WHERE id = $3
RETURNING id, task_id, status, current_phase, progress_percent,
          started_at, completed_at, error_message
