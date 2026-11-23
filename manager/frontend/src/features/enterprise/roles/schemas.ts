import { z } from 'zod';
import { EditionSchema, LicenseStatusSchema } from '../schemas';

export const RoleScope = {
  Organization: 'organization',
  Workspace: 'workspace',
} as const;

export type RoleScope = (typeof RoleScope)[keyof typeof RoleScope];

export const Permission = {
  OrganizationRead: 'organization:read',
  OrganizationUpdate: 'organization:update',
  OrganizationDelete: 'organization:delete',
  MembersCreate: 'members:create',
  MembersUpdate: 'members:update',
  MembersDelete: 'members:delete',
  AdministratorsUpdate: 'administrators:update',
  InvitationsCreate: 'invitations:create',
  InvitationsRead: 'invitations:read',
  InvitationsDelete: 'invitations:delete',
  WorkspacesCreate: 'workspaces:create',
  DevicesRead: 'devices:read',
  DevicesUpdate: 'devices:update',
  GitHubUpdate: 'github:update',
  McpServersUpdate: 'mcp_servers:update',
  AiSettingsRead: 'ai_settings:read',
  AiSettingsUpdate: 'ai_settings:update',
  AgentLoginsUpdate: 'agent_logins:update',
  PoliciesUpdate: 'policies:update',
  AuditLogsRead: 'audit_logs:read',
  AuditUpdate: 'audit:update',
  SpendRead: 'spend:read',
  SpendUpdate: 'spend:update',
  RolesUpdate: 'roles:update',
  ScimUpdate: 'scim:update',
  WorkspaceRead: 'workspace:read',
  WorkspaceWrite: 'workspace:write',
  WorkspaceUpdate: 'workspace:update',
  WorkspaceDelete: 'workspace:delete',
  WorkspaceMembersCreate: 'workspace_members:create',
  WorkspaceMembersUpdate: 'workspace_members:update',
  WorkspaceMembersDelete: 'workspace_members:delete',
  WorkspaceAdministratorsUpdate: 'workspace_administrators:update',
  WorkspaceAiSettingsUpdate: 'workspace_ai_settings:update',
  WorkspaceMcpServersUpdate: 'workspace_mcp_servers:update',
  WorkspacePoliciesUpdate: 'workspace_policies:update',
  ProjectSyncsUpdate: 'project_syncs:update',
  PullRequestsOverride: 'pull_requests:override',
} as const;

export type PermissionName = (typeof Permission)[keyof typeof Permission];

export const RoleCode = {
  LicenseRequired: 'license_required',
  PermissionUnknown: 'permission_unknown',
  PermissionNotComposable: 'permission_not_composable',
  PermissionOutOfScope: 'permission_out_of_scope',
  PermissionRequires: 'permission_requires',
  NameInvalid: 'name_invalid',
  NameReserved: 'name_reserved',
  NameTaken: 'role_name_taken',
  Exceeds: 'role_exceeds',
  InUse: 'role_in_use',
} as const;

export type RoleCode = (typeof RoleCode)[keyof typeof RoleCode];

export const ROLE_SCOPES = [RoleScope.Organization, RoleScope.Workspace] as const;

export const ORGANIZATION_PERMISSIONS = [
  Permission.OrganizationRead,
  Permission.OrganizationUpdate,
  Permission.OrganizationDelete,
  Permission.MembersCreate,
  Permission.MembersUpdate,
  Permission.MembersDelete,
  Permission.AdministratorsUpdate,
  Permission.InvitationsCreate,
  Permission.InvitationsRead,
  Permission.InvitationsDelete,
  Permission.WorkspacesCreate,
  Permission.DevicesRead,
  Permission.DevicesUpdate,
  Permission.GitHubUpdate,
  Permission.McpServersUpdate,
  Permission.AiSettingsRead,
  Permission.AiSettingsUpdate,
  Permission.AgentLoginsUpdate,
  Permission.PoliciesUpdate,
  Permission.AuditLogsRead,
  Permission.AuditUpdate,
  Permission.SpendRead,
  Permission.SpendUpdate,
  Permission.RolesUpdate,
  Permission.ScimUpdate,
] as const;

export const WORKSPACE_PERMISSIONS = [
  Permission.WorkspaceRead,
  Permission.WorkspaceWrite,
  Permission.WorkspaceUpdate,
  Permission.WorkspaceDelete,
  Permission.WorkspaceMembersCreate,
  Permission.WorkspaceMembersUpdate,
  Permission.WorkspaceMembersDelete,
  Permission.WorkspaceAdministratorsUpdate,
  Permission.WorkspaceAiSettingsUpdate,
  Permission.WorkspaceMcpServersUpdate,
  Permission.WorkspacePoliciesUpdate,
  Permission.ProjectSyncsUpdate,
  Permission.PullRequestsOverride,
] as const;

export const PERMISSIONS = [...ORGANIZATION_PERMISSIONS, ...WORKSPACE_PERMISSIONS] as const;

export const ROLE_BASE: Readonly<Record<RoleScope, PermissionName>> = {
  [RoleScope.Organization]: Permission.OrganizationRead,
  [RoleScope.Workspace]: Permission.WorkspaceRead,
};

export const PERMISSION_REQUIREMENTS: Readonly<Partial<Record<PermissionName, PermissionName>>> = {
  [Permission.SpendUpdate]: Permission.SpendRead,
};

export const COMPOSITION_CODES = [
  RoleCode.PermissionUnknown,
  RoleCode.PermissionNotComposable,
  RoleCode.PermissionOutOfScope,
  RoleCode.PermissionRequires,
] as const;

export const ROLE_ROUTE_CODES = [
  RoleCode.NameInvalid,
  RoleCode.NameReserved,
  RoleCode.NameTaken,
  RoleCode.InUse,
] as const;

export const ROLE_CODES = [
  RoleCode.LicenseRequired,
  ...COMPOSITION_CODES,
  ...ROLE_ROUTE_CODES,
  RoleCode.Exceeds,
] as const;

export const RoleScopeSchema = z.enum(ROLE_SCOPES);

export const PermissionNameSchema = z.enum(PERMISSIONS);

export const RoleCodeSchema = z.enum(ROLE_CODES);

export const PermissionDefinitionSchema = z.object({
  name: PermissionNameSchema,
  scope: RoleScopeSchema,
  composable: z.boolean(),
});

export const CustomRoleReferenceSchema = z.object({
  id: z.string(),
  name: z.string(),
});

export const CustomRoleSchema = z.object({
  id: z.string(),
  organization_id: z.string(),
  name: z.string(),
  description: z.string().nullable(),
  scope: RoleScopeSchema,
  permissions: z.array(PermissionNameSchema),
  members: z.number().int().nonnegative(),
  created_at: z.string(),
  updated_at: z.string(),
});

export const RolesResponseSchema = z.object({
  entitled: z.boolean(),
  editable: z.boolean(),
  edition: EditionSchema,
  status: LicenseStatusSchema,
  catalogue: z.array(PermissionDefinitionSchema),
  roles: z.array(CustomRoleSchema),
});

export const RoleResponseSchema = z.object({
  role: CustomRoleSchema,
});

export const CreateRoleRequestSchema = z.strictObject({
  name: z.string(),
  description: z.string().nullable().optional(),
  scope: RoleScopeSchema,
  permissions: z.array(PermissionNameSchema),
});

export const UpdateRoleRequestSchema = z.strictObject({
  name: z.string().optional(),
  description: z.string().nullable().optional(),
  permissions: z.array(PermissionNameSchema).optional(),
});

export const RoleRefusalSchema = z.object({
  error: z.string(),
  code: RoleCodeSchema,
  members: z.number().int().nonnegative().optional(),
});
