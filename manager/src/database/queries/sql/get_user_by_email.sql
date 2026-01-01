-- Get user by email (for login) - returns user with password hash
SELECT id, email, display_name, is_active, is_admin,
       created_at::text, updated_at::text, last_login_at::text,
       password_hash
FROM users
WHERE email = $1
