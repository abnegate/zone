-- Get all sources for a task from source_ids array
SELECT s.id, s.name, s.source_type, s.config, s.credentials_encrypted, s.description, s.url,
       s.is_active, s.last_verified_at::timestamp, s.last_error, s.created_at::timestamp, s.updated_at::timestamp FROM tasks t
CROSS JOIN LATERAL unnest(t.source_ids) AS task_source_id
JOIN sources s ON s.id = task_source_id
WHERE t.id = $1 AND s.is_active = TRUE
