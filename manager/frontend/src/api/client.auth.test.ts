import { afterEach, beforeEach, describe, expect, it, mock } from 'bun:test';
import { client } from './client';

describe('Client - Auth Methods', () => {
  const API_BASE = import.meta.env.VITE_API_URL || '';
  let mockFetch: ReturnType<typeof mock>;

  beforeEach(() => {
    mockFetch = mock();
    global.fetch = mockFetch as typeof fetch;
    client.setAccessToken('test-token');
  });

  afterEach(() => {
    mock.clearAllMocks();
  });

  describe('verifyEmail', () => {
    it('sends POST request to /api/auth/verify-email with token', async () => {
      const mockResponse = {
        success: true,
        message: 'Email verified successfully',
      };

      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: async () => mockResponse,
      });

      const result = await client.verifyEmail('test-token-123');

      expect(global.fetch).toHaveBeenCalledWith(`${API_BASE}/api/auth/verify-email`, {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json',
          Authorization: 'Bearer test-token',
        },
        body: JSON.stringify({ token: 'test-token-123' }),
      });

      expect(result).toEqual(mockResponse);
    });

    it('throws error when verification fails', async () => {
      mockFetch.mockResolvedValueOnce({
        ok: false,
        status: 400,
      });

      await expect(client.verifyEmail('invalid-token')).rejects.toThrow(
        'Failed to verify email: 400'
      );
    });
  });

  describe('resendVerification', () => {
    it('sends POST request to /api/auth/resend-verification with email', async () => {
      const mockResponse = {
        success: true,
        message: 'Verification email sent',
      };

      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: async () => mockResponse,
      });

      const result = await client.resendVerification('test@example.com');

      expect(global.fetch).toHaveBeenCalledWith(`${API_BASE}/api/auth/resend-verification`, {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json',
          Authorization: 'Bearer test-token',
        },
        body: JSON.stringify({ email: 'test@example.com' }),
      });

      expect(result).toEqual(mockResponse);
    });

    it('throws error when resend fails', async () => {
      mockFetch.mockResolvedValueOnce({
        ok: false,
        status: 429,
      });

      await expect(client.resendVerification('test@example.com')).rejects.toThrow(
        'Failed to resend verification email: 429'
      );
    });
  });

  describe('forgotPassword', () => {
    it('sends POST request to /api/auth/forgot-password with email', async () => {
      const mockResponse = {
        success: true,
        message: 'Password reset email sent',
      };

      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: async () => mockResponse,
      });

      const result = await client.forgotPassword('test@example.com');

      expect(global.fetch).toHaveBeenCalledWith(`${API_BASE}/api/auth/forgot-password`, {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json',
        },
        body: JSON.stringify({ email: 'test@example.com' }),
      });

      expect(result).toEqual(mockResponse);
    });

    it('does not include Authorization header for forgot password', async () => {
      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: async () => ({ success: true, message: 'Sent' }),
      });

      await client.forgotPassword('test@example.com');

      const callArgs = mockFetch.mock.calls[0];
      expect(callArgs[1].headers.Authorization).toBeUndefined();
    });

    it('throws error when forgot password fails', async () => {
      mockFetch.mockResolvedValueOnce({
        ok: false,
        status: 404,
      });

      await expect(client.forgotPassword('test@example.com')).rejects.toThrow(
        'Failed to request password reset: 404'
      );
    });
  });

  describe('resetPassword', () => {
    it('sends POST request to /api/auth/reset-password with token and new password', async () => {
      const mockResponse = {
        success: true,
        message: 'Password reset successfully',
      };

      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: async () => mockResponse,
      });

      const result = await client.resetPassword('reset-token-123', 'newPassword123!');

      expect(global.fetch).toHaveBeenCalledWith(`${API_BASE}/api/auth/reset-password`, {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json',
        },
        body: JSON.stringify({
          token: 'reset-token-123',
          new_password: 'newPassword123!',
        }),
      });

      expect(result).toEqual(mockResponse);
    });

    it('does not include Authorization header for reset password', async () => {
      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: async () => ({ success: true, message: 'Reset' }),
      });

      await client.resetPassword('token', 'newpass');

      const callArgs = mockFetch.mock.calls[0];
      expect(callArgs[1].headers.Authorization).toBeUndefined();
    });

    it('throws error when password reset fails', async () => {
      mockFetch.mockResolvedValueOnce({
        ok: false,
        status: 400,
      });

      await expect(client.resetPassword('invalid-token', 'newpass')).rejects.toThrow(
        'Failed to reset password: 400'
      );
    });
  });
});
