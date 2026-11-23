import { z } from 'zod';
import { WorkspaceRoleSchema } from '../../auth/schemas';
import { CustomRoleReferenceSchema } from '../../enterprise/roles/schemas';

export const OrgRoleSchema = z.enum(['owner', 'admin', 'member']);

export const OrganizationSchema = z.object({
  id: z.string(),
  name: z.string(),
  slug: z.string(),
  description: z.string().nullable(),
  is_active: z.boolean(),
  role: OrgRoleSchema.optional(),
  custom_role: CustomRoleReferenceSchema.nullable().optional(),
  permissions: z.array(z.string()).optional(),
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

export const OrganizationMemberSchema = z
  .object({
    id: z.string().min(1),
    user_id: z.string().min(1),
    organization_id: z.string().min(1),
    role: OrgRoleSchema,
    custom_role: CustomRoleReferenceSchema.nullable().default(null),
    permissions: z.array(z.string()).optional(),
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
    custom_role: member.custom_role,
    permissions: member.permissions,
    email: member.email || '',
    display_name: member.display_name ?? null,
    joined_at: member.joined_at || member.created_at || '',
  }));

export const AddOrgMemberRequestSchema = z.object({
  email: z.string().email('Invalid email address'),
  role: OrgRoleSchema,
});

export const CustomRoleAssignmentSchema = z.strictObject({
  custom_role_id: z.string().min(1),
});

export const UpdateOrgMemberRequestSchema = z.union([
  z.strictObject({ role: OrgRoleSchema }),
  CustomRoleAssignmentSchema,
]);

export const OrgMembersResponseSchema = z.object({
  members: z.array(OrganizationMemberSchema),
});

export const DeviceStatusSchema = z.enum(['allowed', 'pending', 'blocked']);
export const DevicePlatformSchema = z.enum(['android', 'ios', 'desktop', 'browser', 'cli']);

export const OrganizationDeviceSchema = z.object({
  id: z.string().min(1),
  user_id: z.string().min(1),
  email: z.string(),
  display_name: z.string().nullable(),
  name: z.string().nullable(),
  platform: DevicePlatformSchema,
  user_agent: z.string().nullable(),
  last_ip: z.string().nullable(),
  last_seen_at: z.string().min(1),
  status: DeviceStatusSchema,
  connected: z.boolean(),
  session_count: z.number(),
  created_at: z.string().min(1),
});

export const DevicesResponseSchema = z.object({
  devices: z.array(OrganizationDeviceSchema),
});

export const DevicePolicySchema = z.object({
  mode: z.enum(['open', 'allowed']),
});

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
  'workspace.owner_seated',
  'settings.updated',
  'settings.reset',
  'device.allowed',
  'device.blocked',
  'device.renamed',
  'device_policy.updated',
  'audit_destination.created',
  'audit_destination.updated',
  'audit_destination.deleted',
  'audit_retention.updated',
  'audit_retention.applied',
  'audit_export.downloaded',
  'role.created',
  'role.updated',
  'role.deleted',
  'scim_token.created',
  'scim_token.rotated',
  'scim_token.revoked',
  'scim_group.mapped',
  'scim_group.unmapped',
  'scim_group.created',
  'scim_group.updated',
  'scim_group.deleted',
  'scim_user.created',
  'scim_user.updated',
  'scim_user.deactivated',
  'scim_user.reactivated',
  'scim_user.deleted',
  'runner_labels.updated',
  'fleet.run_refused',
] as const;
export const AUDIT_RESOURCE_TYPES = [
  'organization',
  'workspace',
  'member',
  'invitation',
  'ai_settings',
  'device',
  'device_policy',
  'audit_destination',
  'audit_retention',
  'audit_export',
  'role',
  'scim_token',
  'scim_group',
  'organization_runner_labels',
  'workspace_runner_labels',
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

export type OrganizationZ = z.infer<typeof OrganizationSchema>;
export type OrganizationMemberZ = z.infer<typeof OrganizationMemberSchema>;
export type OrgMembersResponse = z.infer<typeof OrgMembersResponseSchema>;
export type OrganizationDeviceZ = z.infer<typeof OrganizationDeviceSchema>;
export type DevicesResponse = z.infer<typeof DevicesResponseSchema>;
export type DevicePolicyZ = z.infer<typeof DevicePolicySchema>;
export type InvitationZ = z.infer<typeof InvitationSchema>;
export type InvitationsResponse = z.infer<typeof InvitationsResponseSchema>;
export type AuditActionZ = z.infer<typeof AuditActionSchema>;
export type AuditResourceTypeZ = z.infer<typeof AuditResourceTypeSchema>;
export type AuditLogZ = z.infer<typeof AuditLogSchema>;
export type AuditLogsResponse = z.infer<typeof AuditLogsResponseSchema>;
export type OrganizationsResponse = z.infer<typeof OrganizationsResponseSchema>;
export type OrganizationResponse = z.infer<typeof OrganizationResponseSchema>;
