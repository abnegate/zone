-- Assign a role to a user
INSERT INTO user_roles (user_id, role_id, assigned_by)
SELECT $1, r.id, $2
FROM roles r
WHERE r.name = $3
ON CONFLICT DO NOTHING
