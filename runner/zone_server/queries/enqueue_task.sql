-- Add a task to the execution queue (upsert)
INSERT INTO task_queue (task_id, priority)
VALUES ($1, $2)
ON CONFLICT (task_id) DO UPDATE SET priority = $2, queued_at = NOW()
