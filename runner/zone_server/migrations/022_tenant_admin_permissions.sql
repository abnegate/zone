-- The console has always gated its organization and workspace settings pages on
-- `organizations:update` and `workspaces:update`, and no role could hold either:
-- migration 001 seeded every other resource the console names and skipped these
-- two, so both pages answered "Access Denied" to every user, owners included.
--
-- Which tenant a caller may act on is not this table's business. That is decided
-- per request from `organization_members` and `workspace_members`, so these grants
-- say only that a role administers tenants at all.

INSERT INTO permissions (name, description, resource, action) VALUES
  ('organizations:create', 'Create new organizations', 'organizations', 'create'),
  ('organizations:read', 'View organizations', 'organizations', 'read'),
  ('organizations:update', 'Update organization settings', 'organizations', 'update'),
  ('organizations:delete', 'Delete organizations', 'organizations', 'delete'),
  ('workspaces:create', 'Create new workspaces', 'workspaces', 'create'),
  ('workspaces:read', 'View workspaces', 'workspaces', 'read'),
  ('workspaces:update', 'Update workspace settings', 'workspaces', 'update'),
  ('workspaces:delete', 'Delete workspaces', 'workspaces', 'delete')
ON CONFLICT (name) DO NOTHING;

INSERT INTO role_permissions (role_id, permission_id)
SELECT '00000000-0000-0000-0000-000000000001', id FROM permissions
WHERE resource IN ('organizations', 'workspaces')
ON CONFLICT DO NOTHING;

-- A standard user owns the organization they signed up with, so refusing them
-- the settings pages refuses everyone. Deleting a tenant stays with the roles
-- that administer the deployment.
INSERT INTO role_permissions (role_id, permission_id)
SELECT '00000000-0000-0000-0000-000000000002', id FROM permissions
WHERE name IN (
  'organizations:create', 'organizations:read', 'organizations:update',
  'workspaces:create', 'workspaces:read', 'workspaces:update'
)
ON CONFLICT DO NOTHING;

INSERT INTO role_permissions (role_id, permission_id)
SELECT '00000000-0000-0000-0000-000000000003', id FROM permissions
WHERE resource IN ('organizations', 'workspaces') AND action = 'read'
ON CONFLICT DO NOTHING;
