-- Create a new user
INSERT INTO users (email, password_hash, display_name, is_admin)
VALUES ($1, $2, $3, $4)
RETURNING id, email, display_name, is_active, is_admin,
          created_at::text, updated_at::text, COALESCE(last_login_at::text, '') AS last_login_at
