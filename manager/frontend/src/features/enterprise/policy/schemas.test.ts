import { describe, expect, it } from 'bun:test';
import { OPEN_RULES } from './draft';
import { ACTIVE_ORGANIZATION, ACTIVE_WORKSPACE_OVERRIDDEN, ORGANIZATION_RULES } from './fixtures';
import {
  OrganizationPolicyResponseSchema,
  POLICY_ADMIN_CODES,
  POLICY_AGENTS,
  POLICY_CEILINGS,
  POLICY_DENIAL_CODES,
  POLICY_FIELDS,
  POLICY_PROVIDERS,
  POLICY_TIERS,
  PolicyCode,
  PolicyRefusalSchema,
  PolicyRulesSchema,
  UpdatePolicyRequestSchema,
  WorkspacePolicyResponseSchema,
} from './schemas';

describe('policy vocabulary', () => {
  it('names the providers, agents, tiers and fields as the server serialises them', () => {
    expect(POLICY_PROVIDERS).toEqual([
      'local',
      'anthropic',
      'openai',
      'bedrock',
      'vertex',
      'azure',
      'other',
    ]);
    expect(POLICY_AGENTS).toEqual(['claude', 'codex']);
    expect(POLICY_TIERS).toEqual(['read', 'write', 'host', 'outward']);
    expect(POLICY_CEILINGS).toEqual(['write', 'host']);
    expect(POLICY_FIELDS).toEqual([
      'providers',
      'agents',
      'models',
      'ceiling',
      'mcp_servers',
      'domains',
    ]);
  });

  it('lists the seven denial codes and the seven editing codes', () => {
    expect(POLICY_DENIAL_CODES).toEqual([
      'policy_provider_denied',
      'policy_agent_denied',
      'policy_model_denied',
      'policy_mcp_server_denied',
      'policy_domain_denied',
      'policy_approval_required',
      'policy_unreadable',
    ]);
    expect(POLICY_ADMIN_CODES).toEqual([
      'license_required',
      'policy_wider_than_organization',
      'policy_model_invalid',
      'policy_domain_invalid',
      'policy_mcp_server_invalid',
      'policy_ceiling_invalid',
      'policy_body_invalid',
    ]);
  });
});

describe('PolicyRulesSchema', () => {
  it('accepts rules that restrict nothing and rules that restrict everything', () => {
    expect(PolicyRulesSchema.parse(OPEN_RULES)).toEqual(OPEN_RULES);
    expect(PolicyRulesSchema.parse(ORGANIZATION_RULES)).toEqual(ORGANIZATION_RULES);
  });

  it('requires every key, because the server always serialises all six', () => {
    for (const field of POLICY_FIELDS) {
      const { [field]: _, ...missing } = OPEN_RULES;
      expect(PolicyRulesSchema.safeParse(missing).success, field).toBe(false);
    }
  });

  it('accepts every tier as a ceiling because it mirrors the server type', () => {
    for (const ceiling of POLICY_TIERS) {
      expect(PolicyRulesSchema.safeParse({ ...OPEN_RULES, ceiling }).success, ceiling).toBe(true);
    }
  });

  it('rejects an unknown provider or agent', () => {
    expect(PolicyRulesSchema.safeParse({ ...OPEN_RULES, providers: ['gemini'] }).success).toBe(
      false
    );
    expect(PolicyRulesSchema.safeParse({ ...OPEN_RULES, agents: ['cursor'] }).success).toBe(false);
  });
});

describe('policy responses', () => {
  it('parses the organization and workspace shapes', () => {
    expect(OrganizationPolicyResponseSchema.parse(ACTIVE_ORGANIZATION)).toEqual(
      ACTIVE_ORGANIZATION
    );
    expect(WorkspacePolicyResponseSchema.parse(ACTIVE_WORKSPACE_OVERRIDDEN)).toEqual(
      ACTIVE_WORKSPACE_OVERRIDDEN
    );
  });

  it('rejects a workspace response without its effective rules', () => {
    const { effective: _, ...missing } = ACTIVE_WORKSPACE_OVERRIDDEN;
    expect(WorkspacePolicyResponseSchema.safeParse(missing).success).toBe(false);
  });
});

describe('UpdatePolicyRequestSchema', () => {
  it('lets every key be missing, null or a value', () => {
    expect(UpdatePolicyRequestSchema.parse({})).toEqual({});
    expect(UpdatePolicyRequestSchema.parse({ domains: null, ceiling: 'write' })).toEqual({
      domains: null,
      ceiling: 'write',
    });
  });

  it('rejects a key the server does not know', () => {
    expect(UpdatePolicyRequestSchema.safeParse({ tools: [] }).success).toBe(false);
  });
});

describe('PolicyRefusalSchema', () => {
  it('reads editing refusals and denials', () => {
    for (const code of [...POLICY_ADMIN_CODES, ...POLICY_DENIAL_CODES]) {
      expect(PolicyRefusalSchema.safeParse({ error: 'No', code }).success, code).toBe(true);
    }
  });

  it('rejects a code outside the policy vocabulary', () => {
    expect(PolicyRefusalSchema.safeParse({ error: 'No', code: 'sso_issuer_invalid' }).success).toBe(
      false
    );
    expect(PolicyCode.Wider).toBe('policy_wider_than_organization');
  });
});
