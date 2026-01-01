-- Release a task back to the queue
SELECT (release_task($1, $2) IS NULL) AS success
