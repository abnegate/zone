import {
  InvitationDetailsSchema,
  SessionsResponseSchema,
  VerifyEmailResponseSchema,
} from '../features/auth/schemas';
import {
  AuditLogsResponseSchema,
  LimitsResponseSchema,
  OrganizationSchema,
  UsageResponseSchema,
} from '../features/settings/organization/schemas';
import { getErrors, isValid } from './index';
import {
  ForgotPasswordSchema,
  ResendVerificationRequestSchema,
  ResetPasswordSchema,
  VerifyEmailRequestSchema,
} from './schemas';

describe('Auth Validation Schemas', () => {
  describe('ForgotPasswordSchema', () => {
    it('accepts valid email', () => {
      expect(isValid(ForgotPasswordSchema, { email: 'test@example.com' })).toBe(true);
    });

    it('rejects empty email', () => {
      const errors = getErrors(ForgotPasswordSchema, { email: '' });
      expect(errors.email).toBe('Invalid email address');
    });

    it('rejects invalid email format', () => {
      const errors = getErrors(ForgotPasswordSchema, { email: 'not-an-email' });
      expect(errors.email).toBe('Invalid email address');
    });

    it('rejects missing email', () => {
      const errors = getErrors(ForgotPasswordSchema, {});
      expect(errors.email).toBeTruthy();
    });
  });

  describe('ResetPasswordSchema', () => {
    it('accepts matching passwords', () => {
      expect(
        isValid(ResetPasswordSchema, {
          password: 'Password123',
          confirmPassword: 'Password123',
        })
      ).toBe(true);
    });

    it('rejects password shorter than 8 characters', () => {
      const errors = getErrors(ResetPasswordSchema, {
        password: 'Short1',
        confirmPassword: 'Short1',
      });
      expect(errors.password).toBe('Password must be at least 8 characters');
    });

    it('rejects empty password', () => {
      const errors = getErrors(ResetPasswordSchema, {
        password: '',
        confirmPassword: '',
      });
      expect(errors.password).toBeTruthy();
      expect(errors.password).toContain('Password must');
    });

    it('rejects password without uppercase letter', () => {
      const errors = getErrors(ResetPasswordSchema, {
        password: 'password123',
        confirmPassword: 'password123',
      });
      expect(errors.password).toBe('Password must contain at least one uppercase letter');
    });

    it('rejects password without lowercase letter', () => {
      const errors = getErrors(ResetPasswordSchema, {
        password: 'PASSWORD123',
        confirmPassword: 'PASSWORD123',
      });
      expect(errors.password).toBe('Password must contain at least one lowercase letter');
    });

    it('rejects password without number', () => {
      const errors = getErrors(ResetPasswordSchema, {
        password: 'PasswordABC',
        confirmPassword: 'PasswordABC',
      });
      expect(errors.password).toBe('Password must contain at least one number');
    });

    it('rejects empty confirm password', () => {
      const errors = getErrors(ResetPasswordSchema, {
        password: 'Password123',
        confirmPassword: '',
      });
      expect(errors.confirmPassword).toBeTruthy();
    });

    it('rejects mismatched passwords', () => {
      const errors = getErrors(ResetPasswordSchema, {
        password: 'Password123',
        confirmPassword: 'Different123',
      });
      expect(errors.confirmPassword).toBe('Passwords do not match');
    });

    it('rejects when confirmPassword is missing', () => {
      const errors = getErrors(ResetPasswordSchema, {
        password: 'Password123',
      });
      expect(errors.confirmPassword).toBeTruthy();
    });
  });

  describe('VerifyEmailRequestSchema', () => {
    it('accepts valid token', () => {
      expect(isValid(VerifyEmailRequestSchema, { token: 'abc123' })).toBe(true);
    });

    it('rejects empty token', () => {
      const errors = getErrors(VerifyEmailRequestSchema, { token: '' });
      expect(errors.token).toBe('Token is required');
    });

    it('rejects missing token', () => {
      const errors = getErrors(VerifyEmailRequestSchema, {});
      expect(errors.token).toBeTruthy();
    });
  });

  describe('ResendVerificationRequestSchema', () => {
    it('accepts valid email', () => {
      expect(isValid(ResendVerificationRequestSchema, { email: 'test@example.com' })).toBe(true);
    });

    it('rejects empty email', () => {
      const errors = getErrors(ResendVerificationRequestSchema, { email: '' });
      expect(errors.email).toBe('Invalid email address');
    });

    it('rejects invalid email format', () => {
      const errors = getErrors(ResendVerificationRequestSchema, { email: 'invalid' });
      expect(errors.email).toBe('Invalid email address');
    });

    it('rejects missing email', () => {
      const errors = getErrors(ResendVerificationRequestSchema, {});
      expect(errors.email).toBeTruthy();
    });
  });

  // The bodies the server actually sends, recorded in the live pass of
  // 2026-09-20; the console must read each of them.
  describe('Server response shapes', () => {
    it('reads a message-only auth outcome as success', () => {
      const parsed = VerifyEmailResponseSchema.parse({ message: 'Email verified successfully' });
      expect(parsed.success).toBe(true);
      expect(parsed.message).toBe('Email verified successfully');
    });

    it('reads a session row without a location and with offset timestamps', () => {
      const parsed = SessionsResponseSchema.parse({
        sessions: [
          {
            id: 'session-1',
            user_id: 'user-1',
            ip_address: null,
            user_agent: 'Mozilla/5.0',
            device_info: null,
            last_active_at: '2026-09-20T02:00:00.123456Z',
            expires_at: '2026-09-27T02:00:00Z',
            revoked_at: null,
            created_at: '2026-09-20T02:00:00+00:00',
            updated_at: '2026-09-20T02:00:00+00:00',
            deleted_at: null,
            is_current: true,
          },
        ],
      });
      expect(parsed.sessions[0].location).toBeNull();
      expect(parsed.sessions[0].is_current).toBe(true);
    });

    it('reads invitation details with an offset expiry and an unnamed inviter', () => {
      const parsed = InvitationDetailsSchema.parse({
        organization_name: 'Zone Verify',
        org_role: 'member',
        workspace_role: 'member',
        expires_at: '2026-09-27T02:00:00+00:00',
      });
      expect(parsed.workspace_name).toBeNull();
      expect(parsed.invited_by_email).toBeNull();
    });

    it('reads the role an organization carries for the caller', () => {
      const base = {
        id: 'org-1',
        name: 'Zone Verify',
        slug: 'zone-verify',
        description: null,
        is_active: true,
        created_at: '2026-01-01T00:00:00+00:00',
        updated_at: '2026-01-01T00:00:00+00:00',
      };
      expect(OrganizationSchema.parse({ ...base, role: 'member' }).role).toBe('member');
      expect(OrganizationSchema.parse(base).role).toBeUndefined();
      expect(isValid(OrganizationSchema, { ...base, role: 'viewer' })).toBe(false);
    });

    it('reads audit rows with dotted actions and recorded values', () => {
      const parsed = AuditLogsResponseSchema.parse({
        logs: [
          {
            id: 'log-1',
            organization_id: 'org-1',
            workspace_id: null,
            actor_id: 'user-1',
            actor_email: 'owner@zone.test',
            action: 'member.role_changed',
            resource_type: 'member',
            resource_id: 'user-2',
            old_values: null,
            new_values: { role: 'admin' },
            ip_address: null,
            user_agent: null,
            created_at: '2026-09-20T02:00:00.000001Z',
          },
          {
            id: 'log-2',
            organization_id: 'org-1',
            actor_id: null,
            actor_email: null,
            action: 'settings.reset',
            resource_type: 'ai_settings',
            resource_id: null,
            created_at: '2026-09-20T02:00:00Z',
          },
        ],
        total: 2,
        limit: 50,
        offset: 0,
      });
      expect(parsed.logs[0].new_values).toEqual({ role: 'admin' });
      expect(parsed.logs[1].old_values).toBeNull();
      expect(parsed.logs[1].actor_email).toBeNull();
    });

    it('flattens the metered usage and reads negative limits as unlimited', () => {
      const usage = UsageResponseSchema.parse({
        current_period_start: '2026-09-01T00:00:00Z',
        current_period_end: '2026-10-01T00:00:00Z',
        usage: { chat_messages: 12, members: 2, workspaces: 1 },
      });
      expect(usage).toEqual({
        members: 2,
        workspaces: 1,
        chat_messages: 12,
        period_start: '2026-09-01T00:00:00Z',
        period_end: '2026-10-01T00:00:00Z',
      });

      const limits = LimitsResponseSchema.parse({
        max_workspaces: -1,
        max_members: 3,
        max_chats_per_month: 100,
      });
      expect(limits).toEqual({ max_members: 3, max_workspaces: null, max_chats_per_month: 100 });
    });
  });
});
