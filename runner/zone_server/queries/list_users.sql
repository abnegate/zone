-- List all users
SELECT id, email, display_name, is_active, is_admin,
       created_at::text, updated_at::text, COALESCE(last_login_at::text, '') AS last_login_at
FROM users
ORDER BY created_at DESC
