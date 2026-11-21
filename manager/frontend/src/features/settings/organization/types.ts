import type { WorkspaceRole } from '../../auth/types';
import type { CustomRoleReference } from '../../enterprise/roles/types';

export type {
  AiProvider,
  AiSettings,
  UpdateAiSettingsRequest,
  Workspace,
} from '../workspace/types';

export type OrgRole = 'owner' | 'admin' | 'member';

export interface Organization {
  id: string;
  name: string;
  slug: string;
  description: string | null;
  is_active: boolean;
  role?: OrgRole;
  custom_role?: CustomRoleReference | null;
  permissions?: readonly string[];
  created_at: string;
  updated_at: string;
}

export interface CreateOrganizationRequest {
  name: string;
  slug: string;
  description?: string;
}

export interface UpdateOrganizationRequest {
  name?: string;
  slug?: string;
  description?: string;
  is_active?: boolean;
}

export interface OrganizationMember {
  id: string;
  user_id: string;
  organization_id: string;
  role: OrgRole;
  custom_role: CustomRoleReference | null;
  permissions?: readonly string[];
  email: string;
  display_name: string | null;
  joined_at: string;
}

export interface AddOrgMemberRequest {
  email: string;
  role: OrgRole;
}

export interface CustomRoleAssignment {
  custom_role_id: string;
}

export type UpdateOrgMemberRequest = { role: OrgRole } | CustomRoleAssignment;

export interface OrgMembersResponse {
  members: OrganizationMember[];
}

export type DeviceStatus = 'allowed' | 'pending' | 'blocked';
export type DevicePlatform = 'android' | 'ios' | 'desktop' | 'browser' | 'cli';
export type DevicePolicyMode = 'open' | 'allowed';

export interface OrganizationDevice {
  id: string;
  user_id: string;
  email: string;
  display_name: string | null;
  name: string | null;
  platform: DevicePlatform;
  user_agent: string | null;
  last_ip: string | null;
  last_seen_at: string;
  status: DeviceStatus;
  connected: boolean;
  session_count: number;
  created_at: string;
}

export interface DevicesResponse {
  devices: OrganizationDevice[];
}

export interface DevicePolicy {
  mode: DevicePolicyMode;
}

export interface UpdateDeviceRequest {
  status?: 'allowed' | 'blocked';
  name?: string;
}

export interface Invitation {
  id: string;
  organization_id: string;
  organization_name: string;
  email: string;
  org_role: OrgRole;
  workspace_id: string | null;
  workspace_name: string | null;
  workspace_role: WorkspaceRole | null;
  invited_by_email: string;
  created_at: string;
  expires_at: string;
}

export interface CreateInvitationRequest {
  email: string;
  org_role: OrgRole;
  workspace_id?: string;
  workspace_role?: WorkspaceRole;
}

export interface InvitationsResponse {
  invitations: Invitation[];
}

export interface InvitationAccess {
  read: boolean;
  create: boolean;
  revoke: boolean;
}

// The server names actions and resources with dotted strings (member.added,
// settings.reset); the console filters by the ones it knows and shows the rest.
export type AuditAction = string;
export type AuditResourceType = string;

export interface AuditLog {
  id: string;
  organization_id: string | null;
  workspace_id: string | null;
  actor_id: string | null;
  actor_email: string | null;
  action: AuditAction;
  resource_type: AuditResourceType;
  resource_id: string | null;
  old_values: Record<string, unknown> | null;
  new_values: Record<string, unknown> | null;
  created_at: string;
}

export interface AuditLogFilters {
  action?: AuditAction;
  resource_type?: AuditResourceType;
  resource_id?: string;
  actor_id?: string;
  start_date?: string;
  end_date?: string;
  limit?: number;
  offset?: number;
}

export interface AuditLogsResponse {
  logs: AuditLog[];
  total: number;
}

export interface ApiResponse {
  success: boolean;
  error?: string;
}

export interface OrganizationsResponse extends ApiResponse {
  organizations: Organization[];
}

export interface OrganizationResponse extends ApiResponse {
  organization: Organization;
}
