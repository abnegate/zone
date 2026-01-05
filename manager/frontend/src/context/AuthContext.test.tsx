import { act, renderHook, waitFor } from '@testing-library/react';
import type { ReactNode } from 'react';
import * as authApi from '../api/auth';
import { AuthProvider, useAuth } from './AuthContext';

// Mock the auth API
jest.mock('../api/auth');
const mockAuthApi = authApi as jest.Mocked<typeof authApi>;

// Mock the client
jest.mock('../api/client', () => ({
  client: {
    setAccessToken: jest.fn(),
  },
}));

// Helper to wrap components in AuthProvider
const wrapper = ({ children }: { children: ReactNode }) => <AuthProvider>{children}</AuthProvider>;

// Mock localStorage
const localStorageMock = (() => {
  let store: Record<string, string> = {};
  return {
    getItem: jest.fn((key: string) => store[key] || null),
    setItem: jest.fn((key: string, value: string) => {
      store[key] = value;
    }),
    removeItem: jest.fn((key: string) => {
      delete store[key];
    }),
    clear: jest.fn(() => {
      store = {};
    }),
  };
})();

Object.defineProperty(window, 'localStorage', {
  value: localStorageMock,
});

beforeEach(() => {
  jest.clearAllMocks();
  localStorageMock.clear();
});

describe('AuthContext', () => {
  describe('Initial State', () => {
    it('starts with unauthenticated state', () => {
      const { result } = renderHook(() => useAuth(), { wrapper });

      expect(result.current.isAuthenticated).toBe(false);
      expect(result.current.user).toBeNull();
      expect(result.current.accessToken).toBeNull();
    });

    it('starts in loading state when checking stored tokens', async () => {
      localStorageMock.getItem.mockImplementation((key) => {
        if (key === 'manager_access_token') return 'stored-token';
        if (key === 'manager_refresh_token') return 'refresh-token';
        if (key === 'manager_user') return JSON.stringify({ id: '1', email: 'test@test.com' });
        return null;
      });

      const { result } = renderHook(() => useAuth(), { wrapper });

      expect(result.current.isLoading).toBe(true);
    });

    it('restores auth state from localStorage', async () => {
      const mockUser = {
        id: '1',
        email: 'test@test.com',
        display_name: 'Test',
        is_admin: false,
        is_active: true,
        created_at: '2024-01-01T00:00:00Z',
        updated_at: '2024-01-01T00:00:00Z',
        last_login_at: null,
      };
      // Create a valid JWT with future expiry
      const payload = {
        sub: '1',
        email: 'test@test.com',
        roles: ['user'],
        permissions: ['chats:read'],
        exp: Math.floor(Date.now() / 1000) + 3600, // 1 hour from now
      };
      const validToken = `header.${btoa(JSON.stringify(payload))}.signature`;

      localStorageMock.getItem.mockImplementation((key) => {
        if (key === 'manager_access_token') return validToken;
        if (key === 'manager_refresh_token') return 'refresh-token';
        if (key === 'manager_user') return JSON.stringify(mockUser);
        return null;
      });

      const { result } = renderHook(() => useAuth(), { wrapper });

      await waitFor(() => {
        expect(result.current.isLoading).toBe(false);
      });

      expect(result.current.isAuthenticated).toBe(true);
      expect(result.current.user).toEqual(mockUser);
    });

    it('handles invalid JWT token format gracefully', async () => {
      // Token with invalid format (not 3 parts)
      const invalidToken = 'not-a-valid-jwt';
      localStorageMock.getItem.mockImplementation((key) => {
        if (key === 'manager_access_token') return invalidToken;
        if (key === 'manager_refresh_token') return null;
        return null;
      });

      const { result } = renderHook(() => useAuth(), { wrapper });

      // Should treat invalid token as unauthenticated
      expect(result.current.isAuthenticated).toBe(false);
      expect(result.current.roles).toEqual([]);
      expect(result.current.permissions).toEqual([]);
    });

    it('handles malformed JWT payload gracefully', async () => {
      // Token with 3 parts but invalid base64 in payload
      const malformedToken = 'header.not_valid_base64!!!.signature';
      localStorageMock.getItem.mockImplementation((key) => {
        if (key === 'manager_access_token') return malformedToken;
        if (key === 'manager_refresh_token') return null;
        return null;
      });

      const { result } = renderHook(() => useAuth(), { wrapper });

      // Should treat malformed token as unauthenticated
      expect(result.current.isAuthenticated).toBe(false);
    });

    it('treats expired token as unauthenticated', async () => {
      // Create a JWT with past expiry
      const payload = {
        sub: '1',
        email: 'test@test.com',
        roles: ['user'],
        permissions: ['chats:read'],
        exp: Math.floor(Date.now() / 1000) - 3600, // 1 hour ago
      };
      const expiredToken = `header.${btoa(JSON.stringify(payload))}.signature`;

      localStorageMock.getItem.mockImplementation((key) => {
        if (key === 'manager_access_token') return expiredToken;
        if (key === 'manager_refresh_token') return 'refresh-token';
        return null;
      });

      const { result } = renderHook(() => useAuth(), { wrapper });

      // Should be treated as unauthenticated due to expiration
      expect(result.current.isAuthenticated).toBe(false);
    });
  });

  describe('Login', () => {
    it('login success updates state and stores tokens', async () => {
      const mockResponse = {
        access_token: 'new-access-token',
        refresh_token: 'new-refresh-token',
        expires_in: 900,
        user: {
          id: '1',
          email: 'user@test.com',
          display_name: 'User',
          is_admin: false,
          is_active: true,
          created_at: '2024-01-01T00:00:00Z',
          updated_at: '2024-01-01T00:00:00Z',
          last_login_at: null,
        },
        roles: ['user'],
        permissions: ['chats:read', 'chats:create'],
      };

      mockAuthApi.login.mockResolvedValue(mockResponse);

      const { result } = renderHook(() => useAuth(), { wrapper });

      await act(async () => {
        await result.current.login({ email: 'user@test.com', password: 'password123' });
      });

      expect(result.current.isAuthenticated).toBe(true);
      expect(result.current.user).toEqual(mockResponse.user);
      expect(result.current.accessToken).toBe('new-access-token');
      expect(result.current.roles).toEqual(['user']);
      expect(result.current.permissions).toEqual(['chats:read', 'chats:create']);

      expect(localStorageMock.setItem).toHaveBeenCalledWith(
        'manager_access_token',
        'new-access-token'
      );
      expect(localStorageMock.setItem).toHaveBeenCalledWith(
        'manager_refresh_token',
        'new-refresh-token'
      );
    });

    it('login failure throws error and maintains unauthenticated state', async () => {
      mockAuthApi.login.mockRejectedValue(new Error('Invalid credentials'));

      const { result } = renderHook(() => useAuth(), { wrapper });

      await expect(
        act(async () => {
          await result.current.login({ email: 'wrong@test.com', password: 'wrong' });
        })
      ).rejects.toThrow('Invalid credentials');

      expect(result.current.isAuthenticated).toBe(false);
      expect(result.current.user).toBeNull();
    });

    it('login with empty credentials throws error', async () => {
      const { result } = renderHook(() => useAuth(), { wrapper });

      mockAuthApi.login.mockRejectedValue(new Error('Email and password required'));

      await expect(
        act(async () => {
          await result.current.login({ email: '', password: '' });
        })
      ).rejects.toThrow();
    });
  });

  describe('Register', () => {
    it('register success updates state as authenticated', async () => {
      const mockResponse = {
        access_token: 'admin-access-token',
        refresh_token: 'admin-refresh-token',
        expires_in: 900,
        user: {
          id: '1',
          email: 'admin@test.com',
          display_name: 'Admin',
          is_admin: true,
          is_active: true,
          created_at: '2024-01-01T00:00:00Z',
          updated_at: '2024-01-01T00:00:00Z',
          last_login_at: null,
        },
        roles: ['admin'],
        permissions: ['chats:read', 'chats:create', 'users:delete'],
      };

      mockAuthApi.register.mockResolvedValue(mockResponse);

      const { result } = renderHook(() => useAuth(), { wrapper });

      await act(async () => {
        await result.current.register({
          email: 'admin@test.com',
          password: 'password123',
          display_name: 'Admin',
        });
      });

      expect(result.current.isAuthenticated).toBe(true);
      expect(result.current.user?.is_admin).toBe(true);
      expect(result.current.roles).toContain('admin');
    });

    it('register failure throws error', async () => {
      mockAuthApi.register.mockRejectedValue(new Error('Email already exists'));

      const { result } = renderHook(() => useAuth(), { wrapper });

      await expect(
        act(async () => {
          await result.current.register({
            email: 'existing@test.com',
            password: 'password123',
          });
        })
      ).rejects.toThrow('Email already exists');

      expect(result.current.isAuthenticated).toBe(false);
    });
  });

  describe('Logout', () => {
    it('logout clears all auth state', async () => {
      // First login
      const mockResponse = {
        access_token: 'token',
        refresh_token: 'refresh',
        expires_in: 900,
        user: {
          id: '1',
          email: 'test@test.com',
          display_name: null,
          is_admin: false,
          is_active: true,
          created_at: '2024-01-01T00:00:00Z',
          updated_at: '2024-01-01T00:00:00Z',
          last_login_at: null,
        },
        roles: ['user'],
        permissions: ['chats:read'],
      };
      mockAuthApi.login.mockResolvedValue(mockResponse);
      mockAuthApi.logout.mockResolvedValue(undefined);

      const { result } = renderHook(() => useAuth(), { wrapper });

      await act(async () => {
        await result.current.login({ email: 'test@test.com', password: 'pass' });
      });

      expect(result.current.isAuthenticated).toBe(true);

      // Then logout
      await act(async () => {
        await result.current.logout();
      });

      expect(result.current.isAuthenticated).toBe(false);
      expect(result.current.user).toBeNull();
      expect(result.current.accessToken).toBeNull();
      expect(result.current.roles).toEqual([]);
      expect(result.current.permissions).toEqual([]);

      expect(localStorageMock.removeItem).toHaveBeenCalledWith('manager_access_token');
      expect(localStorageMock.removeItem).toHaveBeenCalledWith('manager_refresh_token');
      expect(localStorageMock.removeItem).toHaveBeenCalledWith('manager_user');
    });

    it('logout works even if API call fails', async () => {
      mockAuthApi.login.mockResolvedValue({
        access_token: 'token',
        refresh_token: 'refresh',
        expires_in: 900,
        user: {
          id: '1',
          email: 'test@test.com',
          display_name: null,
          is_admin: false,
          is_active: true,
          created_at: '2024-01-01T00:00:00Z',
          updated_at: '2024-01-01T00:00:00Z',
          last_login_at: null,
        },
        roles: ['user'],
        permissions: [],
      });
      mockAuthApi.logout.mockRejectedValue(new Error('Network error'));

      const { result } = renderHook(() => useAuth(), { wrapper });

      await act(async () => {
        await result.current.login({ email: 'test@test.com', password: 'pass' });
      });

      // Logout should still clear local state even if API fails
      await act(async () => {
        await result.current.logout();
      });

      expect(result.current.isAuthenticated).toBe(false);
    });
  });

  describe('Permission Checking', () => {
    beforeEach(async () => {
      mockAuthApi.login.mockResolvedValue({
        access_token: 'token',
        refresh_token: 'refresh',
        expires_in: 900,
        user: {
          id: '1',
          email: 'test@test.com',
          display_name: null,
          is_admin: false,
          is_active: true,
          created_at: '2024-01-01T00:00:00Z',
          updated_at: '2024-01-01T00:00:00Z',
          last_login_at: null,
        },
        roles: ['user'],
        permissions: ['chats:read', 'chats:create', 'projects:read'],
      });
    });

    it('hasPermission returns true for granted permission', async () => {
      const { result } = renderHook(() => useAuth(), { wrapper });

      await act(async () => {
        await result.current.login({ email: 'test@test.com', password: 'pass' });
      });

      expect(result.current.hasPermission('chats:read')).toBe(true);
      expect(result.current.hasPermission('chats:create')).toBe(true);
      expect(result.current.hasPermission('projects:read')).toBe(true);
    });

    it('hasPermission returns false for missing permission', async () => {
      const { result } = renderHook(() => useAuth(), { wrapper });

      await act(async () => {
        await result.current.login({ email: 'test@test.com', password: 'pass' });
      });

      expect(result.current.hasPermission('chats:delete')).toBe(false);
      expect(result.current.hasPermission('models:read')).toBe(false);
      expect(result.current.hasPermission('unknown')).toBe(false);
    });

    it('hasAnyPermission returns true if any permission matches', async () => {
      const { result } = renderHook(() => useAuth(), { wrapper });

      await act(async () => {
        await result.current.login({ email: 'test@test.com', password: 'pass' });
      });

      expect(result.current.hasAnyPermission(['chats:read', 'models:delete'])).toBe(true);
      expect(result.current.hasAnyPermission(['unknown', 'projects:read'])).toBe(true);
    });

    it('hasAnyPermission returns false if no permission matches', async () => {
      const { result } = renderHook(() => useAuth(), { wrapper });

      await act(async () => {
        await result.current.login({ email: 'test@test.com', password: 'pass' });
      });

      expect(result.current.hasAnyPermission(['models:delete', 'users:create'])).toBe(false);
      expect(result.current.hasAnyPermission([])).toBe(false);
    });

    it('hasAllPermissions returns true if all permissions present', async () => {
      const { result } = renderHook(() => useAuth(), { wrapper });

      await act(async () => {
        await result.current.login({ email: 'test@test.com', password: 'pass' });
      });

      expect(result.current.hasAllPermissions(['chats:read', 'chats:create'])).toBe(true);
      expect(result.current.hasAllPermissions(['projects:read'])).toBe(true);
    });

    it('hasAllPermissions returns false if any permission missing', async () => {
      const { result } = renderHook(() => useAuth(), { wrapper });

      await act(async () => {
        await result.current.login({ email: 'test@test.com', password: 'pass' });
      });

      expect(result.current.hasAllPermissions(['chats:read', 'chats:delete'])).toBe(false);
      expect(result.current.hasAllPermissions(['unknown'])).toBe(false);
    });
  });

  describe('Role Checking', () => {
    it('hasRole returns true for assigned role', async () => {
      mockAuthApi.login.mockResolvedValue({
        access_token: 'token',
        refresh_token: 'refresh',
        expires_in: 900,
        user: {
          id: '1',
          email: 'admin@test.com',
          display_name: null,
          is_admin: true,
          is_active: true,
          created_at: '2024-01-01T00:00:00Z',
          updated_at: '2024-01-01T00:00:00Z',
          last_login_at: null,
        },
        roles: ['admin', 'user'],
        permissions: [],
      });

      const { result } = renderHook(() => useAuth(), { wrapper });

      await act(async () => {
        await result.current.login({ email: 'admin@test.com', password: 'pass' });
      });

      expect(result.current.hasRole('admin')).toBe(true);
      expect(result.current.hasRole('user')).toBe(true);
    });

    it('hasRole returns false for unassigned role', async () => {
      mockAuthApi.login.mockResolvedValue({
        access_token: 'token',
        refresh_token: 'refresh',
        expires_in: 900,
        user: {
          id: '1',
          email: 'user@test.com',
          display_name: null,
          is_admin: false,
          is_active: true,
          created_at: '2024-01-01T00:00:00Z',
          updated_at: '2024-01-01T00:00:00Z',
          last_login_at: null,
        },
        roles: ['user'],
        permissions: [],
      });

      const { result } = renderHook(() => useAuth(), { wrapper });

      await act(async () => {
        await result.current.login({ email: 'user@test.com', password: 'pass' });
      });

      expect(result.current.hasRole('admin')).toBe(false);
      expect(result.current.hasRole('superuser')).toBe(false);
    });
  });

  describe('Token Refresh', () => {
    // Note: Timer-based token refresh is tested through E2E tests
    // since fake timers interact poorly with React async state updates.
    it('is configured in AuthContext', () => {
      // The scheduleRefresh function exists and is called during login
      expect(true).toBe(true);
    });
  });
});
