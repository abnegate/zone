import {
  type ReactNode,
  createContext,
  useCallback,
  useContext,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
} from 'react';
import * as authApi from '../../../api/auth';
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

export function AuthProvider({ children }: { children: ReactNode }) {
  const [state, setState] = useState<AuthState>(() => {
    const accessToken = localStorage.getItem(ACCESS_TOKEN_KEY);
    const refreshToken = localStorage.getItem(REFRESH_TOKEN_KEY);
    const userJson = localStorage.getItem(USER_KEY);
    const user = userJson ? JSON.parse(userJson) : null;
    const payload = accessToken ? decodeJwt(accessToken) : null;

    // Set client token synchronously to avoid race condition where
    // components fetch before useEffect runs
    client.setAccessToken(accessToken);

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
    client.setAccessToken(state.accessToken);
  }, [state.accessToken]);

  const handleLogout = useCallback(() => {
    if (refreshTimeoutRef.current) {
      clearTimeout(refreshTimeoutRef.current);
    }

    localStorage.removeItem(ACCESS_TOKEN_KEY);
    localStorage.removeItem(REFRESH_TOKEN_KEY);
    localStorage.removeItem(USER_KEY);

    setState({
      user: null,
      roles: [],
      permissions: [],
      accessToken: null,
      refreshToken: null,
      isAuthenticated: false,
      isLoading: false,
    });
  }, []);

  const handleAuthResponse = useCallback((response: AuthResponse) => {
    localStorage.setItem(ACCESS_TOKEN_KEY, response.access_token);
    localStorage.setItem(REFRESH_TOKEN_KEY, response.refresh_token);
    localStorage.setItem(USER_KEY, JSON.stringify(response.user));

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
  }, []);

  const scheduleRefresh = useCallback(
    (expiresIn: number) => {
      if (refreshTimeoutRef.current) {
        clearTimeout(refreshTimeoutRef.current);
      }

      // Refresh 60 seconds before expiry
      const refreshTime = (expiresIn - 60) * 1000;
      if (refreshTime > 0) {
        refreshTimeoutRef.current = setTimeout(async () => {
          const currentRefreshToken = localStorage.getItem(REFRESH_TOKEN_KEY);
          if (currentRefreshToken) {
            try {
              const response = await authApi.refreshToken(currentRefreshToken);
              handleAuthResponse(response);
            } catch {
              handleLogout();
            }
          }
        }, refreshTime);
      }
    },
    [handleAuthResponse, handleLogout]
  );

  // Store scheduleRefresh in ref synchronously before effects run
  useLayoutEffect(() => {
    scheduleRefreshRef.current = scheduleRefresh;
  }, [scheduleRefresh]);

  // Verify/refresh token on mount
  useEffect(() => {
    const verify = async () => {
      const refreshToken = localStorage.getItem(REFRESH_TOKEN_KEY);
      const accessToken = localStorage.getItem(ACCESS_TOKEN_KEY);

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
        const response = await authApi.refreshToken(refreshToken);
        handleAuthResponse(response);
      } catch {
        handleLogout();
      }
    };

    verify();

    return () => {
      if (refreshTimeoutRef.current) {
        clearTimeout(refreshTimeoutRef.current);
      }
    };
  }, [handleAuthResponse, handleLogout, scheduleRefresh]);

  const login = useCallback(
    async (request: LoginRequest) => {
      const response = await authApi.login(request);
      handleAuthResponse(response);
    },
    [handleAuthResponse]
  );

  const register = useCallback(
    async (request: RegisterRequest) => {
      const response = await authApi.register(request);
      handleAuthResponse(response);
    },
    [handleAuthResponse]
  );

  const logout = useCallback(async () => {
    const refreshToken = localStorage.getItem(REFRESH_TOKEN_KEY);
    if (refreshToken) {
      await authApi.logout(refreshToken);
    }
    handleLogout();
  }, [handleLogout]);

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
