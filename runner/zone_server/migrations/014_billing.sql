-- Migration 014: Billing & Subscriptions
-- Plans table
CREATE TABLE IF NOT EXISTS plans (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name VARCHAR(100) NOT NULL,
    slug VARCHAR(50) NOT NULL UNIQUE,
    description TEXT,
    price_monthly_cents INTEGER NOT NULL,
    price_yearly_cents INTEGER NOT NULL,
    is_active BOOLEAN NOT NULL DEFAULT TRUE,
    is_public BOOLEAN NOT NULL DEFAULT TRUE,
    features JSONB NOT NULL DEFAULT '{}',
    limits JSONB NOT NULL DEFAULT '{}',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Subscriptions table
CREATE TABLE IF NOT EXISTS subscriptions (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    organization_id UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    plan_id UUID NOT NULL REFERENCES plans(id),
    status VARCHAR(50) NOT NULL, -- active, past_due, canceled, trialing
    current_period_start TIMESTAMPTZ NOT NULL,
    current_period_end TIMESTAMPTZ NOT NULL,
    cancel_at_period_end BOOLEAN NOT NULL DEFAULT FALSE,
    canceled_at TIMESTAMPTZ,
    trial_start TIMESTAMPTZ,
    trial_end TIMESTAMPTZ,
    stripe_subscription_id VARCHAR(255),
    stripe_customer_id VARCHAR(255),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE(organization_id)
);

-- Usage tracking
CREATE TABLE IF NOT EXISTS usage_events (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    organization_id UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    workspace_id UUID REFERENCES workspaces(id) ON DELETE SET NULL,
    user_id UUID REFERENCES users(id) ON DELETE SET NULL,
    event_type VARCHAR(50) NOT NULL,
    quantity BIGINT NOT NULL DEFAULT 1,
    metadata JSONB DEFAULT '{}',
    recorded_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Indexes
CREATE INDEX IF NOT EXISTS idx_subscriptions_status ON subscriptions(status);
CREATE INDEX IF NOT EXISTS idx_subscriptions_stripe ON subscriptions(stripe_subscription_id);
CREATE INDEX IF NOT EXISTS idx_usage_events_org_time ON usage_events(organization_id, recorded_at);
CREATE INDEX IF NOT EXISTS idx_usage_events_type ON usage_events(event_type);
CREATE INDEX IF NOT EXISTS idx_usage_events_org_type_time ON usage_events(organization_id, event_type, recorded_at);

-- Seed default plans
INSERT INTO plans (name, slug, description, price_monthly_cents, price_yearly_cents, features, limits)
VALUES
    ('Free', 'free', 'For individuals and small teams', 0, 0,
     '{"api_access": true}'::jsonb,
     '{"max_workspaces": 1, "max_members": 3, "max_chats_per_month": 100}'::jsonb),
    ('Pro', 'pro', 'For growing teams', 2900, 29000,
     '{"api_access": true, "priority_support": true}'::jsonb,
     '{"max_workspaces": 10, "max_members": 25, "max_chats_per_month": 5000}'::jsonb),
    ('Enterprise', 'enterprise', 'For large organizations', 9900, 99000,
     '{"api_access": true, "priority_support": true, "sso": true, "audit_log": true}'::jsonb,
     '{"max_workspaces": -1, "max_members": -1, "max_chats_per_month": -1}'::jsonb)
ON CONFLICT (slug) DO NOTHING;
