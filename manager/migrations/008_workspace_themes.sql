-- Workspace theme customization table
-- Stores per-workspace theming configuration for the UI

CREATE TABLE workspace_themes (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  workspace_id UUID NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE UNIQUE,

  -- Light mode colors
  primary_color_light TEXT DEFAULT '#3b82f6',
  secondary_color_light TEXT DEFAULT '#6366f1',

  -- Dark mode colors
  primary_color_dark TEXT DEFAULT '#3b82f6',
  secondary_color_dark TEXT DEFAULT '#6366f1',

  -- Typography (preset fonts only)
  -- Options: 'system', 'inter', 'roboto', 'open-sans', 'lato', 'nunito'
  font_family TEXT DEFAULT 'system',
  font_size_base TEXT DEFAULT '16px',

  -- Corner radius: 'none', 'small', 'medium', 'large'
  border_radius TEXT DEFAULT 'medium',

  created_at TIMESTAMP DEFAULT NOW(),
  updated_at TIMESTAMP DEFAULT NOW()
);

CREATE INDEX idx_workspace_themes_workspace_id ON workspace_themes(workspace_id);
