-- Remove a role from a user
DELETE FROM user_roles
WHERE user_id = $1 AND role_id = (SELECT id FROM roles WHERE name = $2)
