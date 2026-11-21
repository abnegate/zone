import type { z } from 'zod';
import type {
  OrganizationPolicyResponseSchema,
  PolicyAdminCodeSchema,
  PolicyAgentSchema,
  PolicyCeilingSchema,
  PolicyCodeSchema,
  PolicyDenialCodeSchema,
  PolicyFieldSchema,
  PolicyProviderSchema,
  PolicyRefusalSchema,
  PolicyRulesSchema,
  PolicyTierSchema,
  UpdatePolicyRequestSchema,
  WorkspacePolicyResponseSchema,
} from './schemas';

export type PolicyProvider = z.infer<typeof PolicyProviderSchema>;
export type PolicyAgent = z.infer<typeof PolicyAgentSchema>;
export type PolicyTier = z.infer<typeof PolicyTierSchema>;
export type PolicyCeiling = z.infer<typeof PolicyCeilingSchema>;
export type PolicyField = z.infer<typeof PolicyFieldSchema>;
export type PolicyCode = z.infer<typeof PolicyCodeSchema>;
export type PolicyDenialCode = z.infer<typeof PolicyDenialCodeSchema>;
export type PolicyAdminCode = z.infer<typeof PolicyAdminCodeSchema>;
export type PolicyRules = z.infer<typeof PolicyRulesSchema>;
export type OrganizationPolicyResponse = z.infer<typeof OrganizationPolicyResponseSchema>;
export type WorkspacePolicyResponse = z.infer<typeof WorkspacePolicyResponseSchema>;
export type UpdatePolicyRequest = z.infer<typeof UpdatePolicyRequestSchema>;
export type PolicyRefusalBody = z.infer<typeof PolicyRefusalSchema>;
export type PolicyListField = Exclude<PolicyField, 'ceiling'>;
