-- Get source for a task (task source or fallback to project source)
SELECT s.id, s.name, s.source_type, s.config, s.credentials_encrypted, s.description, s.url,
       s.is_active, s.last_verified_at, s.last_error, s.created_at, s.updated_at
FROM tasks t
JOIN projects p ON p.id = t.project_id
LEFT JOIN sources s ON s.id = COALESCE(t.source_id, p.source_id)
WHERE t.id = $1 AND s.is_active = TRUE
