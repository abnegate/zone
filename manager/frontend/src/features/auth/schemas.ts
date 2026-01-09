import { z } from 'zod';

// =============================================================================
// Auth Schemas
// =============================================================================

export const UserSchema = z.object({
  id: z.string(),
  email: z.string().email(),
  email_verified: z.boolean(),
  display_name: z.string().nullable(),
  is_active: z.boolean(),
  is_admin: z.boolean(),
  created_at: z.string(),
  updated_at: z.string(),
  last_login_at: z.string().nullable(),
});

export const AuthResponseSchema = z.object({
  access_token: z.string(),
  refresh_token: z.string(),
  expires_in: z.number(),
  user: UserSchema,
  roles: z.array(z.string()),
  permissions: z.array(z.string()),
});

export const LoginRequestSchema = z.object({
  email: z.string().email('Invalid email address'),
  password: z.string().min(1, 'Password is required'),
});

// Password validation helper
const passwordSchema = z
  .string()
  .min(8, 'Password must be at least 8 characters')
  .regex(/[A-Z]/, 'Password must contain at least one uppercase letter')
  .regex(/[a-z]/, 'Password must contain at least one lowercase letter')
  .regex(/[0-9]/, 'Password must contain at least one number');

export const RegisterRequestSchema = z.object({
  email: z.string().email('Invalid email address'),
  password: passwordSchema,
  display_name: z.string().optional(),
});

export const ForgotPasswordSchema = z.object({
  email: z.string().email('Invalid email address'),
});

export const ResetPasswordSchema = z
  .object({
    password: passwordSchema,
    confirmPassword: z.string().min(1, 'Please confirm your password'),
  })
  .refine((data) => data.password === data.confirmPassword, {
    message: 'Passwords do not match',
    path: ['confirmPassword'],
  });

export const VerifyEmailRequestSchema = z.object({
  token: z.string().min(1, 'Token is required'),
});

export const ResendVerificationRequestSchema = z.object({
  email: z.string().email('Invalid email address'),
});

export const VerifyEmailResponseSchema = z.object({
  success: z.boolean(),
  message: z.string(),
});

export const ResendVerificationResponseSchema = z.object({
  success: z.boolean(),
  message: z.string(),
});

export const ForgotPasswordResponseSchema = z.object({
  success: z.boolean(),
  message: z.string(),
});

export const ResetPasswordResponseSchema = z.object({
  success: z.boolean(),
  message: z.string(),
});

export const JwtPayloadSchema = z.object({
  sub: z.string(),
  email: z.string().email(),
  roles: z.array(z.string()),
  permissions: z.array(z.string()),
  iat: z.number(),
  exp: z.number(),
  jti: z.string(),
});

// =============================================================================
// Session Schemas
// =============================================================================

export const SessionSchema = z.object({
  id: z.string().min(1),
  user_id: z.string().min(1),
  ip_address: z.string().nullable(),
  user_agent: z.string().nullable(),
  device_info: z.string().nullable(),
  location: z.string().nullable(),
  created_at: z.string().datetime(),
  last_active_at: z.string().datetime(),
  expires_at: z.string().datetime(),
  is_current: z.boolean(),
});

export const SessionsResponseSchema = z.object({
  sessions: z.array(SessionSchema),
});

// =============================================================================
// Invitation Schemas (closely related to auth/registration)
// =============================================================================

export const OrgRoleSchema = z.enum(['owner', 'admin', 'member']);
export const WorkspaceRoleSchema = z.enum(['owner', 'admin', 'member', 'viewer']);

export const InvitationSchema = z.object({
  id: z.string().min(1),
  organization_id: z.string().min(1),
  organization_name: z.string(),
  email: z.string().email(),
  org_role: OrgRoleSchema,
  workspace_id: z.string().nullable(),
  workspace_name: z.string().nullable(),
  workspace_role: WorkspaceRoleSchema.nullable(),
  invited_by_email: z.string().email(),
  created_at: z.string().datetime(),
  expires_at: z.string().datetime(),
});

export const InvitationDetailsSchema = z.object({
  organization_name: z.string(),
  org_role: OrgRoleSchema,
  workspace_name: z.string().nullable(),
  workspace_role: WorkspaceRoleSchema.nullable(),
  invited_by_email: z.string().email(),
  expires_at: z.string().datetime(),
});

// =============================================================================
// Type Exports (inferred from schemas)
// =============================================================================

export type UserZ = z.infer<typeof UserSchema>;
export type AuthResponseZ = z.infer<typeof AuthResponseSchema>;
export type LoginRequestZ = z.infer<typeof LoginRequestSchema>;
export type RegisterRequestZ = z.infer<typeof RegisterRequestSchema>;
export type JwtPayloadZ = z.infer<typeof JwtPayloadSchema>;
export type SessionZ = z.infer<typeof SessionSchema>;
export type SessionsResponse = z.infer<typeof SessionsResponseSchema>;
export type OrgRoleZ = z.infer<typeof OrgRoleSchema>;
export type WorkspaceRoleZ = z.infer<typeof WorkspaceRoleSchema>;
export type InvitationZ = z.infer<typeof InvitationSchema>;
export type InvitationDetailsZ = z.infer<typeof InvitationDetailsSchema>;
