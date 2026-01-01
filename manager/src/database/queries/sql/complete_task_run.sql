-- Complete a task run (success or failure)
UPDATE task_runs
SET status = $1, completed_at = $2, error_message = $3,
    progress_percent = CASE WHEN $1 = 'completed' THEN 100 ELSE progress_percent END
WHERE id = $4
RETURNING id, task_id, status, current_phase, progress_percent,
          started_at::timestamp, completed_at::timestamp, error_message
