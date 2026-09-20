import type { WorkspaceRole } from '../../auth/types';

export type {
  AiProvider,
  AiSettings,
  UpdateAiSettingsRequest,
  Workspace,
} from '../workspace/types';

// Organization Types
export type OrgRole = 'owner' | 'admin' | 'member';

export interface Organization {
  id: string;
  name: string;
  slug: string;
  description: string | null;
  is_active: boolean;
  role?: OrgRole;
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

// Organization Member Types
export interface OrganizationMember {
  id: string;
  user_id: string;
  organization_id: string;
  role: OrgRole;
  email: string;
  display_name: string | null;
  joined_at: string;
}

export interface AddOrgMemberRequest {
  email: string;
  role: OrgRole;
}

export interface UpdateOrgMemberRequest {
  role: OrgRole;
}

export interface OrgMembersResponse {
  members: OrganizationMember[];
}

// Invitation Types
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

// Billing & Usage Types
export interface Plan {
  id: string;
  name: string;
  description: string | null;
  price_monthly: number;
  price_yearly: number;
  features: string[];
  limits: PlanLimits;
  is_public: boolean;
}

export interface PlanLimits {
  max_users: number | null;
  max_workspaces: number | null;
  max_projects: number | null;
  max_storage_gb: number | null;
  max_api_calls_monthly: number | null;
}

export interface Subscription {
  id: string;
  organization_id: string;
  plan_id: string;
  plan_name: string;
  status: 'active' | 'canceled' | 'past_due' | 'trialing';
  current_period_start: string;
  current_period_end: string;
  cancel_at_period_end: boolean;
}

export interface Usage {
  members: number;
  workspaces: number;
  chat_messages: number;
  period_start: string;
  period_end: string;
}

export interface Limits {
  max_members: number | null;
  max_workspaces: number | null;
  max_chats_per_month: number | null;
}

export interface PlansResponse {
  plans: Plan[];
}

export interface PlanResponse {
  plan: Plan;
}

export interface SubscriptionResponse {
  subscription: Subscription;
}

export interface UsageResponse extends Usage {}

export interface LimitsResponse extends Limits {}

// Audit Log Types
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

// API Response wrappers
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
