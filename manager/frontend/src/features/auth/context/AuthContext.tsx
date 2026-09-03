import {
  createContext,
  type ReactNode,
  useCallback,
  useContext,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
} from 'react';
import * as authApi from '../../../api/auth';
import { RefreshError } from '../../../api/auth';
import { client } from '../../../api/client';
import type { AuthResponse, JwtPayload, LoginRequest, RegisterRequest, User } from '../types';

interface AuthState {
  user: User | null;
  roles: string[];
  permissions: string[];
  accessToken: string | null;
  refreshToken: string | null;
  isAuthenticated: boolean;
  isLoading: boolean;
}

interface AuthContextType extends AuthState {
  login: (request: LoginRequest) => Promise<void>;
  register: (request: RegisterRequest) => Promise<void>;
  logout: () => Promise<void>;
  hasPermission: (permission: string) => boolean;
  hasAnyPermission: (permissions: string[]) => boolean;
  hasAllPermissions: (permissions: string[]) => boolean;
  hasRole: (role: string) => boolean;
}

const AuthContext = createContext<AuthContextType | undefined>(undefined);

type AuthApi = Pick<typeof authApi, 'login' | 'register' | 'refreshToken' | 'logout'>;
type ClientApi = Pick<typeof client, 'setAccessToken'>;
type StorageApi = Pick<Storage, 'getItem' | 'setItem' | 'removeItem'>;

// Retry window after a refresh that failed for a reason other than the server
// rejecting the credential. scheduleRefresh subtracts 60s, so this lands ~15s out.
const RETRY_REFRESH_SECONDS = 75;

const ACCESS_TOKEN_KEY = 'manager_access_token';
const REFRESH_TOKEN_KEY = 'manager_refresh_token';
const USER_KEY = 'manager_user';

// Decode JWT payload (without verification - that's done server-side)
function decodeJwt(token: string): JwtPayload | null {
  try {
    const parts = token.split('.');
    if (parts.length !== 3) return null;
    const payload = JSON.parse(atob(parts[1]));
    return payload;
  } catch {
    return null;
  }
}

// Check if token is expired (with 30 second buffer)
function isTokenExpired(token: string): boolean {
  const payload = decodeJwt(token);
  if (!payload) return true;
  return payload.exp * 1000 < Date.now() + 30000;
}

export function AuthProvider({
  children,
  authApiOverride,
  clientOverride,
  storageOverride,
}: {
  children: ReactNode;
  authApiOverride?: AuthApi;
  clientOverride?: ClientApi;
  storageOverride?: StorageApi;
}) {
  const auth = authApiOverride ?? authApi;
  const apiClient = clientOverride ?? client;
  const storage = storageOverride ?? localStorage;
  const [state, setState] = useState<AuthState>(() => {
    const accessToken = storage.getItem(ACCESS_TOKEN_KEY);
    const refreshToken = storage.getItem(REFRESH_TOKEN_KEY);
    const userJson = storage.getItem(USER_KEY);
    const user = userJson ? JSON.parse(userJson) : null;
    const payload = accessToken ? decodeJwt(accessToken) : null;

    // Set client token synchronously to avoid race condition where
    // components fetch before useEffect runs
    apiClient.setAccessToken(accessToken);

    return {
      user,
      roles: payload?.roles || [],
      permissions: payload?.permissions || [],
      accessToken,
      refreshToken,
      isAuthenticated: !!accessToken && !isTokenExpired(accessToken),
      isLoading: !!refreshToken, // Will verify on mount
    };
  });

  const refreshTimeoutRef = useRef<NodeJS.Timeout | undefined>(undefined);
  const scheduleRefreshRef = useRef<((expiresIn: number) => void) | null>(null);

  // Update API client token whenever it changes
  useEffect(() => {
    apiClient.setAccessToken(state.accessToken);
  }, [apiClient, state.accessToken]);

  const handleLogout = useCallback(() => {
    if (refreshTimeoutRef.current) {
      clearTimeout(refreshTimeoutRef.current);
    }

    apiClient.setAccessToken(null);

    storage.removeItem(ACCESS_TOKEN_KEY);
    storage.removeItem(REFRESH_TOKEN_KEY);
    storage.removeItem(USER_KEY);

    setState({
      user: null,
      roles: [],
      permissions: [],
      accessToken: null,
      refreshToken: null,
      isAuthenticated: false,
      isLoading: false,
    });
  }, [apiClient, storage]);

  const handleAuthResponse = useCallback(
    (response: AuthResponse) => {
      // Set client token synchronously: WorkspaceProvider is a child of this
      // provider, so its effects run before the effect below syncs the client.
      apiClient.setAccessToken(response.access_token);

      storage.setItem(ACCESS_TOKEN_KEY, response.access_token);
      storage.setItem(REFRESH_TOKEN_KEY, response.refresh_token);
      storage.setItem(USER_KEY, JSON.stringify(response.user));

      setState({
        user: response.user,
        roles: response.roles,
        permissions: response.permissions,
        accessToken: response.access_token,
        refreshToken: response.refresh_token,
        isAuthenticated: true,
        isLoading: false,
      });

      scheduleRefreshRef.current?.(response.expires_in);
    },
    [apiClient, storage]
  );

  const scheduleRefresh = useCallback(
    (expiresIn: number) => {
      if (refreshTimeoutRef.current) {
        clearTimeout(refreshTimeoutRef.current);
      }

      // Refresh 60 seconds before expiry
      const refreshTime = (expiresIn - 60) * 1000;
      if (refreshTime > 0) {
        refreshTimeoutRef.current = setTimeout(async () => {
          const currentRefreshToken = storage.getItem(REFRESH_TOKEN_KEY);
          if (currentRefreshToken) {
            try {
              const response = await auth.refreshToken(currentRefreshToken);
              handleAuthResponse(response);
            } catch (err) {
              // Only a rejected credential ends the session. A proxy reload or a
              // restarting backend must not sign the user out; try again shortly.
              if (err instanceof RefreshError && !err.credentialRejected) {
                scheduleRefreshRef.current?.(RETRY_REFRESH_SECONDS);
                return;
              }
              handleLogout();
            }
          }
        }, refreshTime);
      }
    },
    [auth, handleAuthResponse, handleLogout, storage]
  );

  // Store scheduleRefresh in ref synchronously before effects run
  useLayoutEffect(() => {
    scheduleRefreshRef.current = scheduleRefresh;
  }, [scheduleRefresh]);

  // Verify/refresh token on mount
  useEffect(() => {
    const verify = async () => {
      const refreshToken = storage.getItem(REFRESH_TOKEN_KEY);
      const accessToken = storage.getItem(ACCESS_TOKEN_KEY);

      if (!refreshToken) {
        handleLogout();
        return;
      }

      if (accessToken && !isTokenExpired(accessToken)) {
        const payload = decodeJwt(accessToken);
        if (payload) {
          const remainingTime = payload.exp - Math.floor(Date.now() / 1000);
          scheduleRefresh(remainingTime);
          setState((s) => ({ ...s, isLoading: false }));
          return;
        }
      }

      // Access token expired or invalid, try refresh
      try {
        const response = await auth.refreshToken(refreshToken);
        handleAuthResponse(response);
      } catch (err) {
        if (err instanceof RefreshError && !err.credentialRejected) {
          // Keep the stored session and retry: the server never said no.
          setState((s) => ({ ...s, isLoading: false }));
          scheduleRefreshRef.current?.(RETRY_REFRESH_SECONDS);
          return;
        }
        handleLogout();
      }
    };

    verify();

    return () => {
      if (refreshTimeoutRef.current) {
        clearTimeout(refreshTimeoutRef.current);
      }
    };
  }, [auth, handleAuthResponse, handleLogout, scheduleRefresh, storage]);

  const login = useCallback(
    async (request: LoginRequest) => {
      const response = await auth.login(request);
      handleAuthResponse(response);
    },
    [auth, handleAuthResponse]
  );

  const register = useCallback(
    async (request: RegisterRequest) => {
      const response = await auth.register(request);
      handleAuthResponse(response);
    },
    [auth, handleAuthResponse]
  );

  const logout = useCallback(async () => {
    const refreshToken = storage.getItem(REFRESH_TOKEN_KEY);
    if (refreshToken) {
      await auth.logout(refreshToken);
    }
    handleLogout();
  }, [auth, handleLogout, storage]);

  const hasPermission = useCallback(
    (permission: string) => state.permissions.includes(permission),
    [state.permissions]
  );

  const hasAnyPermission = useCallback(
    (permissions: string[]) => permissions.some((p) => state.permissions.includes(p)),
    [state.permissions]
  );

  const hasAllPermissions = useCallback(
    (permissions: string[]) => permissions.every((p) => state.permissions.includes(p)),
    [state.permissions]
  );

  const hasRole = useCallback((role: string) => state.roles.includes(role), [state.roles]);

  const value = useMemo(
    () => ({
      ...state,
      login,
      register,
      logout,
      hasPermission,
      hasAnyPermission,
      hasAllPermissions,
      hasRole,
    }),
    [state, login, register, logout, hasPermission, hasAnyPermission, hasAllPermissions, hasRole]
  );

  return <AuthContext.Provider value={value}>{children}</AuthContext.Provider>;
}

export function useAuth() {
  const context = useContext(AuthContext);
  if (context === undefined) {
    throw new Error('useAuth must be used within an AuthProvider');
  }
  return context;
}
