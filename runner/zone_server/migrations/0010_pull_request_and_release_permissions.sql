SET LOCAL lock_timeout = '5s';

INSERT INTO permissions (name, description, resource, action) VALUES
  ('pull_requests:read', 'View pull requests', 'pull_requests', 'read'),
  ('pull_requests:update', 'Merge pull requests', 'pull_requests', 'update'),
  ('releases:read', 'View releases', 'releases', 'read'),
  ('releases:create', 'Create releases', 'releases', 'create')
ON CONFLICT (name) DO NOTHING;

INSERT INTO role_permissions (role_id, permission_id)
SELECT '00000000-0000-0000-0000-000000000001', id FROM permissions
WHERE resource IN ('pull_requests', 'releases')
ON CONFLICT DO NOTHING;

INSERT INTO role_permissions (role_id, permission_id)
SELECT '00000000-0000-0000-0000-000000000002', id FROM permissions
WHERE resource IN ('pull_requests', 'releases')
ON CONFLICT DO NOTHING;

INSERT INTO role_permissions (role_id, permission_id)
SELECT '00000000-0000-0000-0000-000000000003', id FROM permissions
WHERE resource IN ('pull_requests', 'releases') AND action = 'read'
ON CONFLICT DO NOTHING;
