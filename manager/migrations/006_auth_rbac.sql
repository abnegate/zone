-- Authentication and RBAC Schema
-- Migration 006

BEGIN;

-- =============================================================================
-- Users Table
-- =============================================================================
CREATE TABLE users (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  email TEXT NOT NULL UNIQUE,
  password_hash TEXT NOT NULL,
  display_name TEXT,
  is_active BOOLEAN DEFAULT TRUE,
  is_admin BOOLEAN DEFAULT FALSE,
  created_at TIMESTAMP WITH TIME ZONE DEFAULT NOW(),
  updated_at TIMESTAMP WITH TIME ZONE DEFAULT NOW(),
  last_login_at TIMESTAMP WITH TIME ZONE
);

CREATE INDEX idx_users_email ON users(email);
CREATE INDEX idx_users_is_active ON users(is_active);

-- =============================================================================
-- Permissions Table
-- =============================================================================
-- Permissions follow the pattern: resource:action
-- Resources: projects, tasks, chats, sources, models, wiki, users
-- Actions: create, read, update, delete
CREATE TABLE permissions (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  name TEXT NOT NULL UNIQUE,
  description TEXT,
  resource TEXT NOT NULL,
  action TEXT NOT NULL CHECK (action IN ('create', 'read', 'update', 'delete')),
  created_at TIMESTAMP WITH TIME ZONE DEFAULT NOW()
);

CREATE INDEX idx_permissions_resource ON permissions(resource);
CREATE INDEX idx_permissions_name ON permissions(name);

-- Seed permissions for all resources
INSERT INTO permissions (name, description, resource, action) VALUES
  -- Projects
  ('projects:create', 'Create new projects', 'projects', 'create'),
  ('projects:read', 'View projects', 'projects', 'read'),
  ('projects:update', 'Update existing projects', 'projects', 'update'),
  ('projects:delete', 'Delete projects', 'projects', 'delete'),
  -- Tasks
  ('tasks:create', 'Create new tasks', 'tasks', 'create'),
  ('tasks:read', 'View tasks', 'tasks', 'read'),
  ('tasks:update', 'Update existing tasks', 'tasks', 'update'),
  ('tasks:delete', 'Delete tasks', 'tasks', 'delete'),
  -- Chats
  ('chats:create', 'Create new chats', 'chats', 'create'),
  ('chats:read', 'View chats', 'chats', 'read'),
  ('chats:update', 'Update existing chats', 'chats', 'update'),
  ('chats:delete', 'Delete chats', 'chats', 'delete'),
  -- Sources
  ('sources:create', 'Create new sources', 'sources', 'create'),
  ('sources:read', 'View sources', 'sources', 'read'),
  ('sources:update', 'Update existing sources', 'sources', 'update'),
  ('sources:delete', 'Delete sources', 'sources', 'delete'),
  -- Models
  ('models:create', 'Pull/install new models', 'models', 'create'),
  ('models:read', 'View installed models', 'models', 'read'),
  ('models:update', 'Update model settings', 'models', 'update'),
  ('models:delete', 'Delete/remove models', 'models', 'delete'),
  -- Wiki
  ('wiki:create', 'Create wiki entries', 'wiki', 'create'),
  ('wiki:read', 'View wiki entries', 'wiki', 'read'),
  ('wiki:update', 'Update wiki entries', 'wiki', 'update'),
  ('wiki:delete', 'Delete wiki entries', 'wiki', 'delete'),
  -- Users (admin only)
  ('users:create', 'Create new users', 'users', 'create'),
  ('users:read', 'View users', 'users', 'read'),
  ('users:update', 'Update users', 'users', 'update'),
  ('users:delete', 'Delete users', 'users', 'delete');

-- =============================================================================
-- Roles Table
-- =============================================================================
CREATE TABLE roles (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  name TEXT NOT NULL UNIQUE,
  description TEXT,
  is_system BOOLEAN DEFAULT FALSE,
  created_at TIMESTAMP WITH TIME ZONE DEFAULT NOW(),
  updated_at TIMESTAMP WITH TIME ZONE DEFAULT NOW()
);

CREATE INDEX idx_roles_name ON roles(name);

-- =============================================================================
-- Role-Permission Join Table
-- =============================================================================
CREATE TABLE role_permissions (
  role_id UUID NOT NULL REFERENCES roles(id) ON DELETE CASCADE,
  permission_id UUID NOT NULL REFERENCES permissions(id) ON DELETE CASCADE,
  PRIMARY KEY (role_id, permission_id)
);

CREATE INDEX idx_role_permissions_role_id ON role_permissions(role_id);

-- =============================================================================
-- User-Role Join Table
-- =============================================================================
CREATE TABLE user_roles (
  user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  role_id UUID NOT NULL REFERENCES roles(id) ON DELETE CASCADE,
  assigned_at TIMESTAMP WITH TIME ZONE DEFAULT NOW(),
  assigned_by UUID REFERENCES users(id),
  PRIMARY KEY (user_id, role_id)
);

CREATE INDEX idx_user_roles_user_id ON user_roles(user_id);
CREATE INDEX idx_user_roles_role_id ON user_roles(role_id);

-- =============================================================================
-- Refresh Tokens Table
-- =============================================================================
CREATE TABLE refresh_tokens (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  token_hash TEXT NOT NULL UNIQUE,
  expires_at TIMESTAMP WITH TIME ZONE NOT NULL,
  created_at TIMESTAMP WITH TIME ZONE DEFAULT NOW(),
  revoked_at TIMESTAMP WITH TIME ZONE,
  user_agent TEXT,
  ip_address TEXT
);

CREATE INDEX idx_refresh_tokens_user_id ON refresh_tokens(user_id);
CREATE INDEX idx_refresh_tokens_token_hash ON refresh_tokens(token_hash);
CREATE INDEX idx_refresh_tokens_expires_at ON refresh_tokens(expires_at);

-- =============================================================================
-- Seed Default Roles
-- =============================================================================

-- Admin role (all permissions)
INSERT INTO roles (id, name, description, is_system)
VALUES ('00000000-0000-0000-0000-000000000001', 'admin', 'Full system access', TRUE);

-- Add all permissions to admin role
INSERT INTO role_permissions (role_id, permission_id)
SELECT '00000000-0000-0000-0000-000000000001', id FROM permissions;

-- User role (standard permissions)
INSERT INTO roles (id, name, description, is_system)
VALUES ('00000000-0000-0000-0000-000000000002', 'user', 'Standard user access', TRUE);

-- Add standard permissions to user role
INSERT INTO role_permissions (role_id, permission_id)
SELECT '00000000-0000-0000-0000-000000000002', id FROM permissions
WHERE name IN (
  'projects:create', 'projects:read', 'projects:update', 'projects:delete',
  'tasks:create', 'tasks:read', 'tasks:update', 'tasks:delete',
  'chats:create', 'chats:read', 'chats:update', 'chats:delete',
  'sources:read',
  'models:read',
  'wiki:read', 'wiki:create', 'wiki:update'
);

-- Viewer role (read-only)
INSERT INTO roles (id, name, description, is_system)
VALUES ('00000000-0000-0000-0000-000000000003', 'viewer', 'Read-only access', TRUE);

INSERT INTO role_permissions (role_id, permission_id)
SELECT '00000000-0000-0000-0000-000000000003', id FROM permissions
WHERE action = 'read';

COMMIT;
