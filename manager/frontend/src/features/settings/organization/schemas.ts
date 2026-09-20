import { z } from 'zod';
import { WorkspaceRoleSchema } from '../../auth/schemas';

// Organization Schemas
export const OrgRoleSchema = z.enum(['owner', 'admin', 'member']);

export const OrganizationSchema = z.object({
  id: z.string(),
  name: z.string(),
  slug: z.string(),
  description: z.string().nullable(),
  is_active: z.boolean(),
  role: OrgRoleSchema.optional(),
  created_at: z.string(),
  updated_at: z.string(),
});

export const CreateOrganizationRequestSchema = z.object({
  name: z.string().min(1, 'Name is required'),
  slug: z.string().min(1, 'Slug is required'),
  description: z.string().optional(),
});

export const UpdateOrganizationRequestSchema = z.object({
  name: z.string().min(1).optional(),
  slug: z.string().min(1).optional(),
  description: z.string().optional(),
  is_active: z.boolean().optional(),
});

export const OrganizationsResponseSchema = z.object({
  success: z.boolean().optional(),
  error: z.string().optional(),
  organizations: z.array(OrganizationSchema),
});

export const OrganizationResponseSchema = z.object({
  success: z.boolean().optional(),
  error: z.string().optional(),
  organization: OrganizationSchema,
});

// Organization Member Schemas
export const OrganizationMemberSchema = z
  .object({
    id: z.string().min(1),
    user_id: z.string().min(1),
    organization_id: z.string().min(1),
    role: OrgRoleSchema,
    is_active: z.boolean().optional(),
    invited_by: z.string().nullable().optional(),
    email: z.string().nullable().optional(),
    display_name: z.string().nullable().optional(),
    joined_at: z.string().optional(),
    created_at: z.string().optional(),
    updated_at: z.string().optional(),
    deleted_at: z.string().nullable().optional(),
  })
  .transform((member) => ({
    id: member.id,
    user_id: member.user_id,
    organization_id: member.organization_id,
    role: member.role,
    email: member.email || '',
    display_name: member.display_name ?? null,
    joined_at: member.joined_at || member.created_at || '',
  }));

export const AddOrgMemberRequestSchema = z.object({
  email: z.string().email('Invalid email address'),
  role: OrgRoleSchema,
});

export const UpdateOrgMemberRequestSchema = z.object({
  role: OrgRoleSchema,
});

export const OrgMembersResponseSchema = z.object({
  members: z.array(OrganizationMemberSchema),
});

// Invitation Schemas
const workspaceRoles = ['owner', 'admin', 'member', 'viewer'] as const;

export const InvitationSchema = z
  .object({
    id: z.string().min(1),
    organization_id: z.string().min(1),
    organization_name: z.string().nullable().optional(),
    email: z.string().min(1),
    org_role: OrgRoleSchema,
    workspace_id: z.string().nullable().optional(),
    workspace_ids: z.array(z.string()).optional(),
    workspace_name: z.string().nullable().optional(),
    workspace_role: z.string().nullable().optional(),
    invited_by: z.string().optional(),
    invited_by_email: z.string().nullable().optional(),
    created_at: z.string().optional(),
    updated_at: z.string().optional(),
    expires_at: z.string(),
    token: z.string().optional(),
    deleted_at: z.string().nullable().optional(),
  })
  .transform((invitation) => {
    const role = invitation.workspace_role;
    return {
      id: invitation.id,
      organization_id: invitation.organization_id,
      organization_name: invitation.organization_name || '',
      email: invitation.email,
      org_role: invitation.org_role,
      workspace_id: invitation.workspace_id ?? invitation.workspace_ids?.[0] ?? null,
      workspace_name: invitation.workspace_name ?? null,
      workspace_role:
        role && workspaceRoles.includes(role as (typeof workspaceRoles)[number])
          ? (role as (typeof workspaceRoles)[number])
          : null,
      invited_by_email: invitation.invited_by_email || '',
      created_at: invitation.created_at || '',
      expires_at: invitation.expires_at,
    };
  });

export const CreateInvitationRequestSchema = z.object({
  email: z.string().email('Invalid email address'),
  org_role: OrgRoleSchema,
  workspace_id: z.string().optional(),
  workspace_role: WorkspaceRoleSchema.optional(),
});

export const InvitationsResponseSchema = z.preprocess(
  (data) => (Array.isArray(data) ? { invitations: data } : data),
  z.object({
    invitations: z.array(InvitationSchema),
  })
);

// Billing & Usage Schemas
export const SubscriptionStatusSchema = z.enum(['active', 'canceled', 'past_due', 'trialing']);

export const PlanLimitsSchema = z.object({
  max_users: z.number().nullable(),
  max_workspaces: z.number().nullable(),
  max_projects: z.number().nullable(),
  max_storage_gb: z.number().nullable(),
  max_api_calls_monthly: z.number().nullable(),
});

export const PlanSchema = z.object({
  id: z.string().min(1),
  name: z.string(),
  description: z.string().nullable(),
  price_monthly: z.number(),
  price_yearly: z.number(),
  features: z.array(z.string()),
  limits: PlanLimitsSchema,
  is_public: z.boolean(),
});

export const SubscriptionSchema = z.object({
  id: z.string().min(1),
  organization_id: z.string().min(1),
  plan_id: z.string().min(1),
  plan_name: z.string(),
  status: SubscriptionStatusSchema,
  current_period_start: z.string().datetime({ offset: true }),
  current_period_end: z.string().datetime({ offset: true }),
  cancel_at_period_end: z.boolean(),
});

export const UsageSchema = z.object({
  members: z.number().min(0),
  workspaces: z.number().min(0),
  chat_messages: z.number().min(0),
  period_start: z.string().datetime({ offset: true }),
  period_end: z.string().datetime({ offset: true }),
});

export const LimitsSchema = z.object({
  max_members: z.number().nullable(),
  max_workspaces: z.number().nullable(),
  max_chats_per_month: z.number().nullable(),
});

export const PlansResponseSchema = z.object({
  plans: z.array(PlanSchema),
});

export const PlanResponseSchema = z.object({
  plan: PlanSchema,
});

export const SubscriptionResponseSchema = z.object({
  subscription: SubscriptionSchema,
});

// The server reports the period and the counters it meters; the console reads
// them as one flat usage record.
export const UsageResponseSchema = z
  .object({
    current_period_start: z.string().datetime({ offset: true }),
    current_period_end: z.string().datetime({ offset: true }),
    usage: z.object({
      chat_messages: z.number().min(0),
      members: z.number().min(0),
      workspaces: z.number().min(0),
    }),
  })
  .transform((response) => ({
    members: response.usage.members,
    workspaces: response.usage.workspaces,
    chat_messages: response.usage.chat_messages,
    period_start: response.current_period_start,
    period_end: response.current_period_end,
  }));

// A plan spells "unlimited" as a negative limit; the console reads it as null.
const boundedLimit = (limit: number): number | null => (limit < 0 ? null : limit);

export const LimitsResponseSchema = z
  .object({
    max_members: z.number(),
    max_workspaces: z.number(),
    max_chats_per_month: z.number(),
  })
  .transform((limits) => ({
    max_members: boundedLimit(limits.max_members),
    max_workspaces: boundedLimit(limits.max_workspaces),
    max_chats_per_month: boundedLimit(limits.max_chats_per_month),
  }));

// Audit Log Schemas
// Actions and resource types are the server's dotted names; the lists name the
// ones the console offers as filters and the schema accepts any the server has.
export const AUDIT_ACTIONS = [
  'member.added',
  'member.removed',
  'member.role_changed',
  'invitation.sent',
  'invitation.accepted',
  'invitation.revoked',
  'organization.updated',
  'organization.deleted',
  'workspace.created',
  'workspace.updated',
  'workspace.deleted',
  'settings.updated',
  'settings.reset',
] as const;
export const AUDIT_RESOURCE_TYPES = [
  'organization',
  'workspace',
  'member',
  'invitation',
  'ai_settings',
] as const;

export const AuditActionSchema = z.string().min(1);
export const AuditResourceTypeSchema = z.string().min(1);

const AuditValuesSchema = z.record(z.string(), z.unknown()).nullable().optional().default(null);

export const AuditLogSchema = z.object({
  id: z.string().min(1),
  organization_id: z.string().nullable(),
  workspace_id: z.string().nullable().optional().default(null),
  actor_id: z.string().nullable(),
  actor_email: z.string().nullable(),
  action: AuditActionSchema,
  resource_type: AuditResourceTypeSchema,
  resource_id: z.string().nullable(),
  old_values: AuditValuesSchema,
  new_values: AuditValuesSchema,
  created_at: z.string().datetime({ offset: true }),
});

export const AuditLogFiltersSchema = z.object({
  action: AuditActionSchema.optional(),
  resource_type: AuditResourceTypeSchema.optional(),
  resource_id: z.string().optional(),
  actor_id: z.string().optional(),
  start_date: z.string().optional(),
  end_date: z.string().optional(),
  limit: z.number().min(1).max(100).optional(),
  offset: z.number().min(0).optional(),
});

export const AuditLogsResponseSchema = z.object({
  logs: z.array(AuditLogSchema),
  total: z.number().min(0),
});

// Type exports
export type OrganizationZ = z.infer<typeof OrganizationSchema>;
export type OrganizationMemberZ = z.infer<typeof OrganizationMemberSchema>;
export type OrgMembersResponse = z.infer<typeof OrgMembersResponseSchema>;
export type InvitationZ = z.infer<typeof InvitationSchema>;
export type InvitationsResponse = z.infer<typeof InvitationsResponseSchema>;
export type PlanZ = z.infer<typeof PlanSchema>;
export type SubscriptionZ = z.infer<typeof SubscriptionSchema>;
export type UsageZ = z.infer<typeof UsageSchema>;
export type LimitsZ = z.infer<typeof LimitsSchema>;
export type PlansResponse = z.infer<typeof PlansResponseSchema>;
export type PlanResponse = z.infer<typeof PlanResponseSchema>;
export type SubscriptionResponse = z.infer<typeof SubscriptionResponseSchema>;
export type UsageResponse = z.infer<typeof UsageResponseSchema>;
export type LimitsResponse = z.infer<typeof LimitsResponseSchema>;
export type AuditActionZ = z.infer<typeof AuditActionSchema>;
export type AuditResourceTypeZ = z.infer<typeof AuditResourceTypeSchema>;
export type AuditLogZ = z.infer<typeof AuditLogSchema>;
export type AuditLogsResponse = z.infer<typeof AuditLogsResponseSchema>;
export type OrganizationsResponse = z.infer<typeof OrganizationsResponseSchema>;
export type OrganizationResponse = z.infer<typeof OrganizationResponseSchema>;
