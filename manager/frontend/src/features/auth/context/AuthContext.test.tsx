import { act, renderHook, waitFor } from '@testing-library/react';
import type { ReactNode } from 'react';
import { beforeEach, describe, expect, it } from 'bun:test';
import { AuthProvider, useAuth } from './AuthContext';
import { RefreshError } from '../../../api/auth';

let loginImpl: (request: { email: string; password: string }) => Promise<unknown>;
let registerImpl: (request: { email: string; password: string }) => Promise<unknown>;
let refreshTokenImpl: (token: string) => Promise<unknown>;
let logoutImpl: (token: string) => Promise<void>;
let setAccessTokenCalls: Array<string | null>;
let storage: {
  getItem: (key: string) => string | null;
  setItem: (key: string, value: string) => void;
  removeItem: (key: string) => void;
};

const createStorage = () => {
  let store: Record<string, string> = {};
  return {
    getItem: (key: string) => store[key] ?? null,
    setItem: (key: string, value: string) => {
      store[key] = value;
    },
    removeItem: (key: string) => {
      delete store[key];
    },
  };
};

const mockAuthApi = {
  login: (request: { email: string; password: string }) => loginImpl(request),
  register: (request: { email: string; password: string }) => registerImpl(request),
  refreshToken: (token: string) => refreshTokenImpl(token),
  logout: (token: string) => logoutImpl(token),
};

const mockClient = {
  setAccessToken: (token: string | null) => {
    setAccessTokenCalls.push(token);
  },
};

const wrapper = ({ children }: { children: ReactNode }) => (
  <AuthProvider authApiOverride={mockAuthApi} clientOverride={mockClient} storageOverride={storage}>
    {children}
  </AuthProvider>
);

const flushEffects = async () => {
  await act(async () => {});
};

beforeEach(() => {
  loginImpl = async () => {
    throw new Error('login not mocked');
  };
  registerImpl = async () => {
    throw new Error('register not mocked');
  };
  refreshTokenImpl = async () => {
    throw new Error('refresh not mocked');
  };
  logoutImpl = async () => {};
  setAccessTokenCalls = [];
  storage = createStorage();
});

describe('AuthContext', () => {
  describe('Initial State', () => {
    it('starts with unauthenticated state', async () => {
      const { result } = renderHook(() => useAuth(), { wrapper });

      expect(result.current.isAuthenticated).toBe(false);
      expect(result.current.user).toBeNull();
      expect(result.current.accessToken).toBeNull();

      await flushEffects();
    });

    it('starts in loading state when checking stored tokens', async () => {
      storage.setItem('manager_access_token', 'stored-token');
      storage.setItem('manager_refresh_token', 'refresh-token');
      storage.setItem(
        'manager_user',
        JSON.stringify({ id: '1', email: 'test@test.com' })
      );
      refreshTokenImpl = () => new Promise(() => {});

      const { result } = renderHook(() => useAuth(), { wrapper });

      expect(result.current.isLoading).toBe(true);

      await flushEffects();
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

      storage.setItem('manager_access_token', validToken);
      storage.setItem('manager_refresh_token', 'refresh-token');
      storage.setItem('manager_user', JSON.stringify(mockUser));

      const { result } = renderHook(() => useAuth(), { wrapper });

      await waitFor(() => {
        expect(result.current.isLoading).toBe(false);
      });

      expect(result.current.isAuthenticated).toBe(true);
      expect(result.current.user).toEqual(mockUser);
    });

    it('handles invalid JWT token format gracefully', async () => {
      const invalidToken = 'not-a-valid-jwt';
      storage.setItem('manager_access_token', invalidToken);

      const { result } = renderHook(() => useAuth(), { wrapper });

      expect(result.current.isAuthenticated).toBe(false);
      expect(result.current.roles).toEqual([]);
      expect(result.current.permissions).toEqual([]);

      await flushEffects();
    });

    it('handles malformed JWT payload gracefully', async () => {
      const malformedToken = 'header.not_valid_base64!!!.signature';
      storage.setItem('manager_access_token', malformedToken);

      const { result } = renderHook(() => useAuth(), { wrapper });

      expect(result.current.isAuthenticated).toBe(false);

      await flushEffects();
    });

    it('treats expired token as unauthenticated', async () => {
      const payload = {
        sub: '1',
        email: 'test@test.com',
        roles: ['user'],
        permissions: ['chats:read'],
        exp: Math.floor(Date.now() / 1000) - 3600,
      };
      const expiredToken = `header.${btoa(JSON.stringify(payload))}.signature`;

      storage.setItem('manager_access_token', expiredToken);
      storage.setItem('manager_refresh_token', 'refresh-token');

      const { result } = renderHook(() => useAuth(), { wrapper });

      expect(result.current.isAuthenticated).toBe(false);

      await flushEffects();
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

      loginImpl = async () => mockResponse;

      const { result } = renderHook(() => useAuth(), { wrapper });

      await act(async () => {
        await result.current.login({ email: 'user@test.com', password: 'password123' });
      });

      expect(result.current.isAuthenticated).toBe(true);
      expect(result.current.user).toEqual(mockResponse.user);
      expect(result.current.accessToken).toBe('new-access-token');
      expect(result.current.roles).toEqual(['user']);
      expect(result.current.permissions).toEqual(['chats:read', 'chats:create']);

      expect(storage.getItem('manager_access_token')).toBe('new-access-token');
      expect(storage.getItem('manager_refresh_token')).toBe('new-refresh-token');
    });

    it('login failure throws error and maintains unauthenticated state', async () => {
      loginImpl = async () => {
        throw new Error('Invalid credentials');
      };

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

      loginImpl = async () => {
        throw new Error('Email and password required');
      };

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

      registerImpl = async () => mockResponse;

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
      registerImpl = async () => {
        throw new Error('Email already exists');
      };

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

      loginImpl = async () => mockResponse;

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

      expect(storage.getItem('manager_access_token')).toBeNull();
      expect(storage.getItem('manager_refresh_token')).toBeNull();
      expect(storage.getItem('manager_user')).toBeNull();
    });
  });

  describe('refresh failures', () => {
    const makeExpiredToken = () => {
      const payload = {
        sub: '1',
        email: 'test@test.com',
        roles: ['user'],
        permissions: ['chats:read'],
        exp: Math.floor(Date.now() / 1000) - 3600,
      };
      return `header.${btoa(JSON.stringify(payload))}.signature`;
    };

    // A proxy reload or a restarting backend used to sign the user out, because
    // every refresh rejection was treated as a rejected credential.
    it('keeps the stored session when the server never rejected the token', async () => {
      storage.setItem('manager_access_token', makeExpiredToken());
      storage.setItem('manager_refresh_token', 'refresh-token');
      refreshTokenImpl = async () => {
        throw new RefreshError('Token refresh failed: 404', 404);
      };

      const { result } = renderHook(() => useAuth(), { wrapper });

      await waitFor(() => {
        expect(result.current.isLoading).toBe(false);
      });

      expect(storage.getItem('manager_refresh_token')).toBe('refresh-token');
    });

    it('signs out when the server rejects the refresh token', async () => {
      storage.setItem('manager_access_token', makeExpiredToken());
      storage.setItem('manager_refresh_token', 'refresh-token');
      refreshTokenImpl = async () => {
        throw new RefreshError('Token refresh failed: 401', 401);
      };

      const { result } = renderHook(() => useAuth(), { wrapper });

      await waitFor(() => {
        expect(storage.getItem('manager_refresh_token')).toBeNull();
      });

      expect(result.current.isAuthenticated).toBe(false);
    });
  });
});
