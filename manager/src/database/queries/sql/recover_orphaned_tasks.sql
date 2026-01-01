-- Recover orphaned tasks (called on worker startup)
SELECT recover_orphaned_tasks()
