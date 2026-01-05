import { login, logout, refreshToken, register } from './auth';

// Mock fetch
global.fetch = jest.fn();
const mockFetch = global.fetch as jest.Mock;

describe('auth API', () => {
  beforeEach(() => {
    jest.clearAllMocks();
  });

  describe('login', () => {
    it('returns auth response on success', async () => {
      const mockResponse = {
        access_token: 'test-access-token',
        refresh_token: 'test-refresh-token',
        expires_in: 3600,
        user: {
          id: '1',
          email: 'test@test.com',
          display_name: null,
          is_active: true,
          is_admin: false,
          created_at: '2024-01-01T00:00:00Z',
          updated_at: '2024-01-01T00:00:00Z',
          last_login_at: null,
        },
        roles: ['user'],
        permissions: ['read'],
      };

      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: () => Promise.resolve({ data: mockResponse }),
      });

      const result = await login({ email: 'test@test.com', password: 'password' });

      expect(result).toEqual(mockResponse);
      expect(mockFetch).toHaveBeenCalledWith('/api/auth/login', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ email: 'test@test.com', password: 'password' }),
      });
    });

    it('throws error on failure', async () => {
      mockFetch.mockResolvedValueOnce({
        ok: false,
        json: () => Promise.resolve({ error: 'Invalid credentials' }),
      });

      await expect(login({ email: 'test@test.com', password: 'wrong' })).rejects.toThrow(
        'Invalid credentials'
      );
    });

    it('uses default error message when json parsing fails', async () => {
      mockFetch.mockResolvedValueOnce({
        ok: false,
        json: () => Promise.reject(new Error('JSON parse error')),
      });

      await expect(login({ email: 'test@test.com', password: 'wrong' })).rejects.toThrow(
        'Login failed'
      );
    });
  });

  describe('register', () => {
    it('returns auth response on success', async () => {
      const mockResponse = {
        access_token: 'test-access-token',
        refresh_token: 'test-refresh-token',
        expires_in: 3600,
        user: {
          id: '1',
          email: 'new@test.com',
          display_name: null,
          is_active: true,
          is_admin: false,
          created_at: '2024-01-01T00:00:00Z',
          updated_at: '2024-01-01T00:00:00Z',
          last_login_at: null,
        },
        roles: ['user'],
        permissions: ['read'],
      };

      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: () => Promise.resolve({ data: mockResponse }),
      });

      const result = await register({
        email: 'new@test.com',
        password: 'password',
      });

      expect(result).toEqual(mockResponse);
      expect(mockFetch).toHaveBeenCalledWith('/api/auth/register', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          email: 'new@test.com',
          password: 'password',
        }),
      });
    });

    it('throws error on failure', async () => {
      mockFetch.mockResolvedValueOnce({
        ok: false,
        json: () => Promise.resolve({ error: 'Email already exists' }),
      });

      await expect(
        register({
          email: 'existing@test.com',
          password: 'password',
        })
      ).rejects.toThrow('Email already exists');
    });

    it('uses default error message when json parsing fails', async () => {
      mockFetch.mockResolvedValueOnce({
        ok: false,
        json: () => Promise.reject(new Error('JSON parse error')),
      });

      await expect(
        register({
          email: 'test@test.com',
          password: 'password',
        })
      ).rejects.toThrow('Registration failed');
    });
  });

  describe('refreshToken', () => {
    it('returns auth response on success', async () => {
      const mockResponse = {
        access_token: 'new-access-token',
        refresh_token: 'new-refresh-token',
        expires_in: 3600,
        user: {
          id: '1',
          email: 'test@test.com',
          display_name: null,
          is_active: true,
          is_admin: false,
          created_at: '2024-01-01T00:00:00Z',
          updated_at: '2024-01-01T00:00:00Z',
          last_login_at: null,
        },
        roles: ['user'],
        permissions: ['read'],
      };

      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: () => Promise.resolve({ data: mockResponse }),
      });

      const result = await refreshToken('old-refresh-token');

      expect(result).toEqual(mockResponse);
      expect(mockFetch).toHaveBeenCalledWith('/api/auth/refresh', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ refresh_token: 'old-refresh-token' }),
      });
    });

    it('throws error on failure', async () => {
      mockFetch.mockResolvedValueOnce({
        ok: false,
      });

      await expect(refreshToken('invalid-token')).rejects.toThrow('Token refresh failed');
    });
  });

  describe('logout', () => {
    it('calls logout endpoint', async () => {
      mockFetch.mockResolvedValueOnce({
        ok: true,
      });

      await logout('refresh-token');

      expect(mockFetch).toHaveBeenCalledWith('/api/auth/logout', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ refresh_token: 'refresh-token' }),
      });
    });

    it('ignores logout errors', async () => {
      mockFetch.mockRejectedValueOnce(new Error('Network error'));

      // Should not throw
      await expect(logout('refresh-token')).resolves.toBeUndefined();
    });
  });
});
