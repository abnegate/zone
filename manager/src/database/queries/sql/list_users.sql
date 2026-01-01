-- List all users
SELECT id, email, display_name, is_active, is_admin,
       created_at::text, updated_at::text, last_login_at::text
FROM users
ORDER BY created_at DESC
