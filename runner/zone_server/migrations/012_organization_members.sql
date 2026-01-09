-- Organization membership: User <-> Organization relationship
CREATE TABLE organization_members (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    organization_id UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    role TEXT NOT NULL DEFAULT 'member' CHECK (role IN ('owner', 'admin', 'member')),
    is_active BOOLEAN NOT NULL DEFAULT TRUE,
    invited_by UUID REFERENCES users(id) ON DELETE SET NULL,
    invited_at TIMESTAMP,
    accepted_at TIMESTAMP,
    created_at TIMESTAMP DEFAULT NOW(),
    updated_at TIMESTAMP DEFAULT NOW(),
    UNIQUE(organization_id, user_id)
);

CREATE INDEX idx_org_members_org ON organization_members(organization_id);
CREATE INDEX idx_org_members_user ON organization_members(user_id);
CREATE INDEX idx_org_members_active ON organization_members(organization_id, is_active) WHERE is_active = TRUE;
CREATE INDEX idx_org_members_role ON organization_members(organization_id, role);

-- Helper function: Check organization membership
CREATE OR REPLACE FUNCTION check_organization_membership(
    p_user_id UUID,
    p_organization_id UUID
) RETURNS BOOLEAN AS $$
BEGIN
    RETURN EXISTS(
        SELECT 1 FROM organization_members
        WHERE user_id = p_user_id
        AND organization_id = p_organization_id
        AND is_active = TRUE
    );
END;
$$ LANGUAGE plpgsql;

-- Helper function: Get organization role
CREATE OR REPLACE FUNCTION get_organization_role(
    p_user_id UUID,
    p_organization_id UUID
) RETURNS TEXT AS $$
DECLARE
    v_role TEXT;
BEGIN
    SELECT role INTO v_role
    FROM organization_members
    WHERE user_id = p_user_id
    AND organization_id = p_organization_id
    AND is_active = TRUE;

    RETURN v_role;
END;
$$ LANGUAGE plpgsql;

-- Update workspace_members to add invited_at and accepted_at columns if they don't exist
DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM information_schema.columns
        WHERE table_name = 'workspace_members' AND column_name = 'invited_at'
    ) THEN
        ALTER TABLE workspace_members ADD COLUMN invited_at TIMESTAMP;
    END IF;

    IF NOT EXISTS (
        SELECT 1 FROM information_schema.columns
        WHERE table_name = 'workspace_members' AND column_name = 'accepted_at'
    ) THEN
        ALTER TABLE workspace_members ADD COLUMN accepted_at TIMESTAMP;
    END IF;
END $$;

-- Seed default organization membership for default user
-- (assumes default user from migration 006 exists)
INSERT INTO organization_members (organization_id, user_id, role, accepted_at)
SELECT
    '00000000-0000-0000-0000-000000000001', -- default organization
    id,
    'owner',
    NOW()
FROM users
WHERE id = '00000000-0000-0000-0000-000000000001'
ON CONFLICT DO NOTHING;
