import { act, renderHook, waitFor } from '@testing-library/react';
import type { ReactNode } from 'react';
import { afterAll, beforeAll, beforeEach, describe, expect, it, mock } from 'bun:test';

const mockLogin = mock();
const mockRegister = mock();
const mockRefreshToken = mock();
const mockLogout = mock();
const mockSetAccessToken = mock();

mock.module('../../../api/auth', () => ({
  login: mockLogin,
  register: mockRegister,
  refreshToken: mockRefreshToken,
  logout: mockLogout,
}));

mock.module('../../../api/client', () => ({
  client: {
    setAccessToken: mockSetAccessToken,
  },
}));

let AuthProvider: typeof import('./AuthContext').AuthProvider;
let useAuth: typeof import('./AuthContext').useAuth;

beforeAll(async () => {
  const authContext = await import('./AuthContext');
  AuthProvider = authContext.AuthProvider;
  useAuth = authContext.useAuth;
});

afterAll(() => {
  mock.restore();
});

const wrapper = ({ children }: { children: ReactNode }) => <AuthProvider>{children}</AuthProvider>;

beforeEach(() => {
  mockLogin.mockReset();
  mockRegister.mockReset();
  mockRefreshToken.mockReset();
  mockLogout.mockReset();
  mockSetAccessToken.mockReset();
  window.localStorage.clear();
});

describe('AuthContext', () => {
  describe('Initial State', () => {
    it('starts with unauthenticated state', () => {
      const { result } = renderHook(() => useAuth(), { wrapper });

      expect(result.current.isAuthenticated).toBe(false);
      expect(result.current.user).toBeNull();
      expect(result.current.accessToken).toBeNull();
    });

    it('starts in loading state when checking stored tokens', () => {
      window.localStorage.setItem('manager_access_token', 'stored-token');
      window.localStorage.setItem('manager_refresh_token', 'refresh-token');
      window.localStorage.setItem(
        'manager_user',
        JSON.stringify({ id: '1', email: 'test@test.com' })
      );
      mockRefreshToken.mockImplementation(() => new Promise(() => {}));

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
        email_verified: true,
        created_at: '2024-01-01T00:00:00Z',
        updated_at: '2024-01-01T00:00:00Z',
        last_login_at: null,
      };
      const payload = {
        sub: '1',
        email: 'test@test.com',
        roles: ['user'],
        permissions: ['chats:read'],
        exp: Math.floor(Date.now() / 1000) + 3600,
      };
      const validToken = `header.${btoa(JSON.stringify(payload))}.signature`;

      window.localStorage.setItem('manager_access_token', validToken);
      window.localStorage.setItem('manager_refresh_token', 'refresh-token');
      window.localStorage.setItem('manager_user', JSON.stringify(mockUser));

      const { result } = renderHook(() => useAuth(), { wrapper });

      await waitFor(() => {
        expect(result.current.isLoading).toBe(false);
      });

      expect(result.current.isAuthenticated).toBe(true);
      expect(result.current.user).toEqual(mockUser);
    });

    it('handles invalid JWT token format gracefully', () => {
      const invalidToken = 'not-a-valid-jwt';
      window.localStorage.setItem('manager_access_token', invalidToken);

      const { result } = renderHook(() => useAuth(), { wrapper });

      expect(result.current.isAuthenticated).toBe(false);
      expect(result.current.roles).toEqual([]);
      expect(result.current.permissions).toEqual([]);
    });

    it('handles malformed JWT payload gracefully', () => {
      const malformedToken = 'header.not_valid_base64!!!.signature';
      window.localStorage.setItem('manager_access_token', malformedToken);

      const { result } = renderHook(() => useAuth(), { wrapper });

      expect(result.current.isAuthenticated).toBe(false);
    });

    it('treats expired token as unauthenticated', () => {
      const payload = {
        sub: '1',
        email: 'test@test.com',
        roles: ['user'],
        permissions: ['chats:read'],
        exp: Math.floor(Date.now() / 1000) - 3600,
      };
      const expiredToken = `header.${btoa(JSON.stringify(payload))}.signature`;

      window.localStorage.setItem('manager_access_token', expiredToken);
      window.localStorage.setItem('manager_refresh_token', 'refresh-token');

      const { result } = renderHook(() => useAuth(), { wrapper });

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
          email_verified: true,
          created_at: '2024-01-01T00:00:00Z',
          updated_at: '2024-01-01T00:00:00Z',
          last_login_at: null,
        },
        roles: ['user'],
        permissions: ['chats:read', 'chats:create'],
      };

      mockLogin.mockResolvedValue(mockResponse);

      const { result } = renderHook(() => useAuth(), { wrapper });

      await act(async () => {
        await result.current.login({ email: 'user@test.com', password: 'password123' });
      });

      expect(result.current.isAuthenticated).toBe(true);
      expect(result.current.user).toEqual(mockResponse.user);
      expect(result.current.accessToken).toBe('new-access-token');
      expect(result.current.roles).toEqual(['user']);
      expect(result.current.permissions).toEqual(['chats:read', 'chats:create']);

      expect(window.localStorage.getItem('manager_access_token')).toBe('new-access-token');
      expect(window.localStorage.getItem('manager_refresh_token')).toBe('new-refresh-token');
    });

    it('login failure throws error and maintains unauthenticated state', async () => {
      mockLogin.mockRejectedValue(new Error('Invalid credentials'));

      const { result } = renderHook(() => useAuth(), { wrapper });

      let error: unknown;
      await act(async () => {
        try {
          await result.current.login({ email: 'wrong@test.com', password: 'wrong' });
        } catch (err) {
          error = err;
        }
      });

      expect(error).toBeInstanceOf(Error);
      expect((error as Error).message).toBe('Invalid credentials');

      expect(result.current.isAuthenticated).toBe(false);
      expect(result.current.user).toBeNull();
    });

    it('login with empty credentials throws error', async () => {
      const { result } = renderHook(() => useAuth(), { wrapper });

      mockLogin.mockRejectedValue(new Error('Email and password required'));

      let error: unknown;
      await act(async () => {
        try {
          await result.current.login({ email: '', password: '' });
        } catch (err) {
          error = err;
        }
      });

      expect(error).toBeInstanceOf(Error);
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
          email_verified: true,
          created_at: '2024-01-01T00:00:00Z',
          updated_at: '2024-01-01T00:00:00Z',
          last_login_at: null,
        },
        roles: ['admin'],
        permissions: ['chats:read', 'chats:create', 'users:delete'],
      };

      mockRegister.mockResolvedValue(mockResponse);

      const { result } = renderHook(() => useAuth(), { wrapper });

      await act(async () => {
        await result.current.register({
          email: 'admin@test.com',
          password: 'password123',
        });
      });

      expect(result.current.isAuthenticated).toBe(true);
      expect(result.current.user).toEqual(mockResponse.user);
      expect(result.current.roles).toEqual(['admin']);
      expect(result.current.permissions).toEqual(['chats:read', 'chats:create', 'users:delete']);
    });

    it('register failure throws error and maintains unauthenticated state', async () => {
      mockRegister.mockRejectedValue(new Error('Email already exists'));

      const { result } = renderHook(() => useAuth(), { wrapper });

      let error: unknown;
      await act(async () => {
        try {
          await result.current.register({ email: 'existing@test.com', password: 'password' });
        } catch (err) {
          error = err;
        }
      });

      expect(error).toBeInstanceOf(Error);
      expect((error as Error).message).toBe('Email already exists');

      expect(result.current.isAuthenticated).toBe(false);
      expect(result.current.user).toBeNull();
    });
  });

  describe('Logout', () => {
    it('logout clears state and storage', async () => {
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
          email_verified: true,
          created_at: '2024-01-01T00:00:00Z',
          updated_at: '2024-01-01T00:00:00Z',
          last_login_at: null,
        },
        roles: ['user'],
        permissions: ['chats:read', 'chats:create'],
      };

      mockLogin.mockResolvedValue(mockResponse);

      const { result } = renderHook(() => useAuth(), { wrapper });

      await act(async () => {
        await result.current.login({ email: 'user@test.com', password: 'password123' });
      });

      await act(async () => {
        await result.current.logout();
      });

      expect(result.current.isAuthenticated).toBe(false);
      expect(result.current.user).toBeNull();
      expect(result.current.accessToken).toBeNull();
      expect(result.current.refreshToken).toBeNull();

      expect(window.localStorage.getItem('manager_access_token')).toBeNull();
      expect(window.localStorage.getItem('manager_refresh_token')).toBeNull();
      expect(window.localStorage.getItem('manager_user')).toBeNull();
    });
  });
});
