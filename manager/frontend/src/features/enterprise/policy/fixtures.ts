import { Edition, LicenseStatus } from '../schemas';
import { OPEN_RULES } from './draft';
import { PolicyAgent, PolicyProvider, PolicyTier } from './schemas';
import type { OrganizationPolicyResponse, PolicyRules, WorkspacePolicyResponse } from './types';

export const ORGANIZATION_ID = '9a8b7c6d-5e4f-4a3b-8c2d-1e0f9a8b7c6d';

export const WORKSPACE_ID = '1f2e3d4c-5b6a-4978-8a6b-5c4d3e2f1a0b';

export const UPDATED_AT = '2026-11-15T10:30:00Z';

export const ORGANIZATION_RULES: PolicyRules = {
  providers: [PolicyProvider.Local, PolicyProvider.Anthropic, PolicyProvider.Bedrock],
  agents: [PolicyAgent.Claude],
  models: ['claude-*', 'qwen3'],
  ceiling: PolicyTier.Host,
  mcp_servers: null,
  domains: ['github.com', 'example.com'],
};

export const WORKSPACE_RULES: PolicyRules = {
  ...OPEN_RULES,
  providers: [PolicyProvider.Local],
  ceiling: PolicyTier.Write,
  domains: ['github.com'],
};

export const COMMUNITY_ORGANIZATION: OrganizationPolicyResponse = {
  entitled: false,
  enforced: false,
  edition: Edition.Community,
  status: LicenseStatus.Community,
  editable: false,
  policy: OPEN_RULES,
  updated_at: null,
};

export const ACTIVE_ORGANIZATION: OrganizationPolicyResponse = {
  entitled: true,
  enforced: true,
  edition: Edition.Enterprise,
  status: LicenseStatus.Active,
  editable: true,
  policy: ORGANIZATION_RULES,
  updated_at: UPDATED_AT,
};

export const LAPSED_ORGANIZATION: OrganizationPolicyResponse = {
  entitled: false,
  enforced: true,
  edition: Edition.Team,
  status: LicenseStatus.Lapsed,
  editable: false,
  policy: ORGANIZATION_RULES,
  updated_at: UPDATED_AT,
};

export const COMMUNITY_WORKSPACE: WorkspacePolicyResponse = {
  entitled: false,
  enforced: false,
  edition: Edition.Community,
  status: LicenseStatus.Community,
  editable: false,
  organization: OPEN_RULES,
  workspace: OPEN_RULES,
  effective: OPEN_RULES,
  updated_at: null,
};

export const ACTIVE_WORKSPACE_INHERITED: WorkspacePolicyResponse = {
  entitled: true,
  enforced: true,
  edition: Edition.Enterprise,
  status: LicenseStatus.Active,
  editable: true,
  organization: ORGANIZATION_RULES,
  workspace: OPEN_RULES,
  effective: ORGANIZATION_RULES,
  updated_at: null,
};

export const ACTIVE_WORKSPACE_OVERRIDDEN: WorkspacePolicyResponse = {
  ...ACTIVE_WORKSPACE_INHERITED,
  workspace: WORKSPACE_RULES,
  effective: {
    ...ORGANIZATION_RULES,
    providers: [PolicyProvider.Local],
    ceiling: PolicyTier.Write,
    domains: ['github.com'],
  },
  updated_at: UPDATED_AT,
};
