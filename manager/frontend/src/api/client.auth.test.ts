import { client } from './client';

describe('Client - Auth Methods', () => {
  const API_BASE = import.meta.env.VITE_API_URL || '';

  beforeEach(() => {
    global.fetch = jest.fn();
    client.setAccessToken('test-token');
  });

  afterEach(() => {
    jest.resetAllMocks();
  });

  describe('verifyEmail', () => {
    it('sends POST request to /api/auth/verify-email with token', async () => {
      const mockResponse = {
        success: true,
        message: 'Email verified successfully',
      };

      (global.fetch as jest.Mock).mockResolvedValueOnce({
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
      (global.fetch as jest.Mock).mockResolvedValueOnce({
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

      (global.fetch as jest.Mock).mockResolvedValueOnce({
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
      (global.fetch as jest.Mock).mockResolvedValueOnce({
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

      (global.fetch as jest.Mock).mockResolvedValueOnce({
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
      (global.fetch as jest.Mock).mockResolvedValueOnce({
        ok: true,
        json: async () => ({ success: true, message: 'Sent' }),
      });

      await client.forgotPassword('test@example.com');

      const callArgs = (global.fetch as jest.Mock).mock.calls[0];
      expect(callArgs[1].headers.Authorization).toBeUndefined();
    });

    it('throws error when forgot password fails', async () => {
      (global.fetch as jest.Mock).mockResolvedValueOnce({
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

      (global.fetch as jest.Mock).mockResolvedValueOnce({
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
      (global.fetch as jest.Mock).mockResolvedValueOnce({
        ok: true,
        json: async () => ({ success: true, message: 'Reset' }),
      });

      await client.resetPassword('token', 'newpass');

      const callArgs = (global.fetch as jest.Mock).mock.calls[0];
      expect(callArgs[1].headers.Authorization).toBeUndefined();
    });

    it('throws error when password reset fails', async () => {
      (global.fetch as jest.Mock).mockResolvedValueOnce({
        ok: false,
        status: 400,
      });

      await expect(client.resetPassword('invalid-token', 'newpass')).rejects.toThrow(
        'Failed to reset password: 400'
      );
    });
  });
});
