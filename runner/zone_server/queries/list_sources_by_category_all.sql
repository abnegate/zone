-- List sources by category (all)
SELECT s.id, s.name, s.source_type, s.config, s.credentials_encrypted, s.description, s.url,
       s.is_active, s.last_verified_at::timestamp, s.last_error, s.created_at::timestamp, s.updated_at::timestamp FROM sources s
JOIN source_types st ON st.name = s.source_type
WHERE st.category = $1
ORDER BY s.name ASC
