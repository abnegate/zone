-- Organizations and Workspaces
-- Migration 006
-- Adds organizational hierarchy: Organization -> Workspace -> Project/Chat/Task

BEGIN;

-- Organizations table (top-level resource)
CREATE TABLE organizations (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  name TEXT NOT NULL,
  slug TEXT NOT NULL UNIQUE,
  description TEXT,
  is_active BOOLEAN DEFAULT TRUE,
  created_at TIMESTAMP WITH TIME ZONE DEFAULT NOW(),
  updated_at TIMESTAMP WITH TIME ZONE DEFAULT NOW()
);

CREATE INDEX idx_organizations_slug ON organizations(slug);
CREATE INDEX idx_organizations_active ON organizations(is_active) WHERE is_active = TRUE;

-- Workspaces table (nested under organizations)
CREATE TABLE workspaces (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  organization_id UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
  name TEXT NOT NULL,
  slug TEXT NOT NULL,
  description TEXT,
  is_active BOOLEAN DEFAULT TRUE,
  created_at TIMESTAMP WITH TIME ZONE DEFAULT NOW(),
  updated_at TIMESTAMP WITH TIME ZONE DEFAULT NOW(),
  UNIQUE(organization_id, slug)
);

CREATE INDEX idx_workspaces_org_id ON workspaces(organization_id);
CREATE INDEX idx_workspaces_slug ON workspaces(slug);
CREATE INDEX idx_workspaces_active ON workspaces(is_active) WHERE is_active = TRUE;

-- Add workspace_id to existing tables
ALTER TABLE projects ADD COLUMN workspace_id UUID REFERENCES workspaces(id) ON DELETE CASCADE;
ALTER TABLE chats ADD COLUMN workspace_id UUID REFERENCES workspaces(id) ON DELETE CASCADE;
ALTER TABLE tasks ADD COLUMN workspace_id UUID REFERENCES workspaces(id) ON DELETE CASCADE;

-- Indexes for workspace filtering
CREATE INDEX idx_projects_workspace_id ON projects(workspace_id);
CREATE INDEX idx_chats_workspace_id ON chats(workspace_id);
CREATE INDEX idx_tasks_workspace_id ON tasks(workspace_id);

-- Create default organization and workspace for self-hosted installations
INSERT INTO organizations (id, name, slug, description, is_active)
VALUES (
  '00000000-0000-0000-0000-000000000001',
  'Default Organization',
  'default',
  'Default organization for self-hosted installation',
  true
);

INSERT INTO workspaces (id, organization_id, name, slug, description, is_active)
VALUES (
  '00000000-0000-0000-0000-000000000001',
  '00000000-0000-0000-0000-000000000001',
  'Default Workspace',
  'default',
  'Default workspace for self-hosted installation',
  true
);

-- Record migration
INSERT INTO schema_migrations (version) VALUES (6);

COMMIT;
