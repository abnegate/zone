import { getErrors, isValid } from './index';
import {
  ForgotPasswordSchema,
  ResetPasswordSchema,
  VerifyEmailRequestSchema,
  ResendVerificationRequestSchema,
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
});
