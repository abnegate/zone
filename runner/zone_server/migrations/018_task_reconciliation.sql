-- Admission is already fenced. Lock live runs after their tasks before choosing
-- winners, so a completion that was waiting/in progress is observed after the
-- lock wait instead of being overwritten from an earlier statement snapshot.
SELECT tasks.id FROM tasks WHERE EXISTS (SELECT 1 FROM task_runs WHERE task_runs.task_id = tasks.id AND task_runs.status = 'running') ORDER BY tasks.id FOR UPDATE;
SELECT id FROM task_runs WHERE status = 'running' ORDER BY task_id, id FOR UPDATE;

WITH ranked AS (
    SELECT id, ROW_NUMBER() OVER (PARTITION BY task_id ORDER BY started_at DESC NULLS LAST, id DESC) AS position
    FROM task_runs WHERE status = 'running'
)
UPDATE task_runs SET status = 'failed', error_message = 'superseded', completed_at = NOW()
WHERE status = 'running' AND id IN (SELECT id FROM ranked WHERE position > 1);

UPDATE tasks SET active_run_id = task_runs.id, status = 'in_progress'
FROM task_runs WHERE task_runs.task_id = tasks.id AND task_runs.status = 'running';
