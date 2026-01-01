-- Delete an organization by ID
DELETE FROM organizations
WHERE id = $1
