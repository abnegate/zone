SET LOCAL lock_timeout = '5s';

UPDATE tasks t SET priority = 5
FROM task_automation a
WHERE a.task_id = t.id AND a.auto_created AND t.priority = 1 AND t.status NOT IN ('complete', 'cancelled');
