// Auth Types
export interface User {
  id: string;
  email: string;
  email_verified: boolean;
  display_name: string | null;
  is_active: boolean;
  is_admin: boolean;
  created_at: string;
  updated_at: string;
  last_login_at: string | null;
}

export interface AuthResponse {
  access_token: string;
  refresh_token: string;
  expires_in: number;
  user: User;
  roles: string[];
  permissions: string[];
}

export interface LoginRequest {
  email: string;
  password: string;
}

export interface RegisterRequest {
  email: string;
  password: string;
  display_name?: string;
}

export interface VerifyEmailRequest {
  token: string;
}

export interface VerifyEmailResponse {
  success: boolean;
  message: string;
}

export interface ResendVerificationRequest {
  email: string;
}

export interface ResendVerificationResponse {
  success: boolean;
  message: string;
}

export interface ForgotPasswordRequest {
  email: string;
}

export interface ForgotPasswordResponse {
  success: boolean;
  message: string;
}

export interface ResetPasswordRequest {
  token: string;
  new_password: string;
}

export interface ResetPasswordResponse {
  success: boolean;
  message: string;
}

export interface JwtPayload {
  sub: string;
  email: string;
  roles: string[];
  permissions: string[];
  iat: number;
  exp: number;
  jti: string;
}

// Session Types
export interface Session {
  id: string;
  user_id: string;
  ip_address: string | null;
  user_agent: string | null;
  device_info: string | null;
  location: string | null;
  created_at: string;
  last_active_at: string;
  expires_at: string;
  is_current: boolean;
}

export interface SessionsResponse {
  sessions: Session[];
}

// Invitation Types
export type OrgRole = 'owner' | 'admin' | 'member';
export type WorkspaceRole = 'owner' | 'admin' | 'member' | 'viewer';

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

export interface InvitationDetails {
  organization_name: string;
  org_role: OrgRole;
  workspace_name: string | null;
  workspace_role: WorkspaceRole | null;
  invited_by_email: string;
  expires_at: string;
}
