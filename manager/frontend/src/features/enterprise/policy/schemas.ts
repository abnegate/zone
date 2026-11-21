import { z } from 'zod';
import { EditionSchema, LicenseStatusSchema } from '../schemas';

export const PolicyProvider = {
  Local: 'local',
  Anthropic: 'anthropic',
  OpenAI: 'openai',
  Bedrock: 'bedrock',
  Vertex: 'vertex',
  Azure: 'azure',
  Other: 'other',
} as const;

export const PolicyAgent = {
  Claude: 'claude',
  Codex: 'codex',
} as const;

export const PolicyTier = {
  Read: 'read',
  Write: 'write',
  Host: 'host',
  Outward: 'outward',
} as const;

export const PolicyField = {
  Providers: 'providers',
  Agents: 'agents',
  Models: 'models',
  Ceiling: 'ceiling',
  McpServers: 'mcp_servers',
  Domains: 'domains',
} as const;

export const PolicyCode = {
  ProviderDenied: 'policy_provider_denied',
  AgentDenied: 'policy_agent_denied',
  ModelDenied: 'policy_model_denied',
  ServerDenied: 'policy_mcp_server_denied',
  DomainDenied: 'policy_domain_denied',
  ApprovalRequired: 'policy_approval_required',
  Unreadable: 'policy_unreadable',
  LicenseRequired: 'license_required',
  Wider: 'policy_wider_than_organization',
  ModelInvalid: 'policy_model_invalid',
  DomainInvalid: 'policy_domain_invalid',
  ServerInvalid: 'policy_mcp_server_invalid',
  CeilingInvalid: 'policy_ceiling_invalid',
  BodyInvalid: 'policy_body_invalid',
} as const;

export const POLICY_PROVIDERS = [
  PolicyProvider.Local,
  PolicyProvider.Anthropic,
  PolicyProvider.OpenAI,
  PolicyProvider.Bedrock,
  PolicyProvider.Vertex,
  PolicyProvider.Azure,
  PolicyProvider.Other,
] as const;

export const POLICY_AGENTS = [PolicyAgent.Claude, PolicyAgent.Codex] as const;

export const POLICY_TIERS = [
  PolicyTier.Read,
  PolicyTier.Write,
  PolicyTier.Host,
  PolicyTier.Outward,
] as const;

export const POLICY_CEILINGS = [PolicyTier.Write, PolicyTier.Host] as const;

export const POLICY_FIELDS = [
  PolicyField.Providers,
  PolicyField.Agents,
  PolicyField.Models,
  PolicyField.Ceiling,
  PolicyField.McpServers,
  PolicyField.Domains,
] as const;

export const POLICY_DENIAL_CODES = [
  PolicyCode.ProviderDenied,
  PolicyCode.AgentDenied,
  PolicyCode.ModelDenied,
  PolicyCode.ServerDenied,
  PolicyCode.DomainDenied,
  PolicyCode.ApprovalRequired,
  PolicyCode.Unreadable,
] as const;

export const POLICY_ADMIN_CODES = [
  PolicyCode.LicenseRequired,
  PolicyCode.Wider,
  PolicyCode.ModelInvalid,
  PolicyCode.DomainInvalid,
  PolicyCode.ServerInvalid,
  PolicyCode.CeilingInvalid,
  PolicyCode.BodyInvalid,
] as const;

export const POLICY_CODES = [...POLICY_ADMIN_CODES, ...POLICY_DENIAL_CODES] as const;

export const PolicyProviderSchema = z.enum(POLICY_PROVIDERS);

export const PolicyAgentSchema = z.enum(POLICY_AGENTS);

export const PolicyTierSchema = z.enum(POLICY_TIERS);

export const PolicyCeilingSchema = z.enum(POLICY_CEILINGS);

export const PolicyFieldSchema = z.enum(POLICY_FIELDS);

export const PolicyDenialCodeSchema = z.enum(POLICY_DENIAL_CODES);

export const PolicyAdminCodeSchema = z.enum(POLICY_ADMIN_CODES);

export const PolicyCodeSchema = z.enum(POLICY_CODES);

export const PolicyRulesSchema = z.object({
  providers: z.array(PolicyProviderSchema).nullable(),
  agents: z.array(PolicyAgentSchema).nullable(),
  models: z.array(z.string()).nullable(),
  ceiling: PolicyTierSchema.nullable(),
  mcp_servers: z.array(z.string()).nullable(),
  domains: z.array(z.string()).nullable(),
});

const LicenseFields = {
  entitled: z.boolean(),
  enforced: z.boolean(),
  edition: EditionSchema,
  status: LicenseStatusSchema,
  editable: z.boolean(),
};

export const OrganizationPolicyResponseSchema = z.object({
  ...LicenseFields,
  policy: PolicyRulesSchema,
  updated_at: z.string().nullable(),
});

export const WorkspacePolicyResponseSchema = z.object({
  ...LicenseFields,
  organization: PolicyRulesSchema,
  workspace: PolicyRulesSchema,
  effective: PolicyRulesSchema,
  updated_at: z.string().nullable(),
});

export const UpdatePolicyRequestSchema = z.strictObject({
  providers: z.array(PolicyProviderSchema).nullable().optional(),
  agents: z.array(PolicyAgentSchema).nullable().optional(),
  models: z.array(z.string()).nullable().optional(),
  ceiling: PolicyCeilingSchema.nullable().optional(),
  mcp_servers: z.array(z.string()).nullable().optional(),
  domains: z.array(z.string()).nullable().optional(),
});

export const PolicyRefusalSchema = z.object({
  error: z.string(),
  code: PolicyCodeSchema,
});
