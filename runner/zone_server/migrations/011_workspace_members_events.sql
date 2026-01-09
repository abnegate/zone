-- Workspace members: User <-> Workspace relationship
CREATE TABLE workspace_members (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    workspace_id UUID NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    role TEXT NOT NULL DEFAULT 'member' CHECK (role IN ('owner', 'admin', 'member', 'viewer')),
    is_active BOOLEAN NOT NULL DEFAULT TRUE,
    invited_by UUID REFERENCES users(id) ON DELETE SET NULL,
    created_at TIMESTAMP DEFAULT NOW(),
    updated_at TIMESTAMP DEFAULT NOW(),
    UNIQUE(workspace_id, user_id)
);

CREATE INDEX idx_workspace_members_workspace ON workspace_members(workspace_id);
CREATE INDEX idx_workspace_members_user ON workspace_members(user_id);
CREATE INDEX idx_workspace_members_active ON workspace_members(workspace_id, is_active) WHERE is_active = TRUE;

-- Gathering events: For WebSocket streaming
CREATE TABLE gathering_events (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    gathering_id UUID NOT NULL REFERENCES context_gatherings(id) ON DELETE CASCADE,
    event_type TEXT NOT NULL,
    payload JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMP DEFAULT NOW()
);

CREATE INDEX idx_gathering_events_gathering ON gathering_events(gathering_id);
CREATE INDEX idx_gathering_events_created ON gathering_events(created_at);
CREATE INDEX idx_gathering_events_gathering_created ON gathering_events(gathering_id, created_at);

-- Add user_id to context_gatherings for audit trail
ALTER TABLE context_gatherings ADD COLUMN user_id UUID REFERENCES users(id) ON DELETE SET NULL;
CREATE INDEX idx_context_gatherings_user ON context_gatherings(user_id);

-- Helper function: Check workspace membership
CREATE OR REPLACE FUNCTION check_workspace_membership(
    p_user_id UUID,
    p_workspace_id UUID
) RETURNS BOOLEAN AS $$
BEGIN
    RETURN EXISTS(
        SELECT 1 FROM workspace_members
        WHERE user_id = p_user_id
        AND workspace_id = p_workspace_id
        AND is_active = TRUE
    );
END;
$$ LANGUAGE plpgsql;

-- Helper function: Check workspace role (for admin operations)
CREATE OR REPLACE FUNCTION get_workspace_role(
    p_user_id UUID,
    p_workspace_id UUID
) RETURNS TEXT AS $$
DECLARE
    v_role TEXT;
BEGIN
    SELECT role INTO v_role
    FROM workspace_members
    WHERE user_id = p_user_id
    AND workspace_id = p_workspace_id
    AND is_active = TRUE;

    RETURN v_role;
END;
$$ LANGUAGE plpgsql;

-- Seed default workspace membership for default user
-- (assumes default user from migration 006 exists)
INSERT INTO workspace_members (workspace_id, user_id, role)
SELECT
    '00000000-0000-0000-0000-000000000001', -- default workspace
    id,
    'owner'
FROM users
WHERE id = '00000000-0000-0000-0000-000000000001'
ON CONFLICT DO NOTHING;
