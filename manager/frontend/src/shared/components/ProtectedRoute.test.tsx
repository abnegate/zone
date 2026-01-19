import { render, screen } from '@testing-library/react';
import { afterAll, beforeAll, beforeEach, describe, expect, it, mock } from 'bun:test';

// Mock react-router-dom
let mockCurrentRoute = '/';
const mockNavigate = () => {};
mock.module('react-router-dom', () => ({
  BrowserRouter: ({ children }: { children: React.ReactNode }) => <>{children}</>,
  Link: ({ to, children }: { to: string; children: React.ReactNode }) => (
    <a href={to}>{children}</a>
  ),
  NavLink: ({ to, children }: { to: string; children: React.ReactNode }) => (
    <a href={to}>{children}</a>
  ),
  Navigate: ({ to }: { to: string; replace?: boolean }) => {
    mockCurrentRoute = to;
    return null;
  },
  useNavigate: () => mockNavigate,
  useLocation: () => ({ pathname: mockCurrentRoute, state: null }),
  useSearchParams: () => [new URLSearchParams(), mock()],
}));

let ProtectedRoute: typeof import('./ProtectedRoute').default;

beforeAll(async () => {
  ProtectedRoute = (await import('./ProtectedRoute')).default;
});

afterAll(() => {
  mock.restore();
});

type AuthState = {
  isAuthenticated: boolean;
  isLoading: boolean;
  user: {
    id: string;
    email: string;
    display_name: string | null;
    is_admin: boolean;
    is_active: boolean;
    email_verified: boolean;
    created_at: string;
    updated_at: string;
    last_login_at: string | null;
  } | null;
  accessToken: string | null;
  refreshToken: string | null;
  roles: string[];
  permissions: string[];
  login: () => Promise<void>;
  register: () => Promise<void>;
  logout: () => Promise<void>;
  hasPermission: (permission: string) => boolean;
  hasAnyPermission: (permissions: string[]) => boolean;
  hasAllPermissions: (permissions: string[]) => boolean;
  hasRole: (role: string) => boolean;
};

const createAuthState = (overrides: Partial<AuthState> = {}): AuthState => ({
  isAuthenticated: false,
  isLoading: false,
  user: null,
  accessToken: null,
  refreshToken: null,
  roles: [],
  permissions: [],
  login: async () => {},
  register: async () => {},
  logout: async () => {},
  hasPermission: () => false,
  hasAnyPermission: () => false,
  hasAllPermissions: () => false,
  hasRole: () => false,
  ...overrides,
});

let authState = createAuthState();
const useAuthHook = () => authState;

// Test components
const ProtectedContent = () => <div data-testid="protected-content">Protected Content</div>;

// Helper to render
const renderProtectedRoute = (ui: React.ReactElement) => {
  mockCurrentRoute = '/';
  return render(ui);
};

describe('ProtectedRoute', () => {
  beforeEach(() => {
    mockCurrentRoute = '/';
    authState = createAuthState();
  });

  describe('Authentication', () => {
    it('shows loading spinner while auth is loading', () => {
      authState = createAuthState({
        isAuthenticated: false,
        isLoading: true,
      });

      renderProtectedRoute(
        <ProtectedRoute useAuthHook={useAuthHook}>
          <ProtectedContent />
        </ProtectedRoute>
      );

      expect(screen.getByText('Loading...')).toBeInTheDocument();
      expect(screen.queryByTestId('protected-content')).not.toBeInTheDocument();
    });

    it('redirects to login when not authenticated', () => {
      authState = createAuthState({
        isAuthenticated: false,
        isLoading: false,
      });

      renderProtectedRoute(
        <ProtectedRoute useAuthHook={useAuthHook}>
          <ProtectedContent />
        </ProtectedRoute>
      );

      expect(mockCurrentRoute).toBe('/login');
      expect(screen.queryByTestId('protected-content')).not.toBeInTheDocument();
    });

    it('renders children when authenticated', () => {
      authState = createAuthState({
        isAuthenticated: true,
        isLoading: false,
        user: {
          id: '1',
          email: 'test@test.com',
          display_name: 'Test',
          is_admin: false,
          is_active: true,
          email_verified: true,
          created_at: '2024-01-01T00:00:00Z',
          updated_at: '2024-01-01T00:00:00Z',
          last_login_at: null,
        },
        accessToken: 'token',
        refreshToken: 'refresh',
        roles: ['user'],
      });

      renderProtectedRoute(
        <ProtectedRoute useAuthHook={useAuthHook}>
          <ProtectedContent />
        </ProtectedRoute>
      );

      expect(screen.getByTestId('protected-content')).toBeInTheDocument();
    });
  });

  describe('Single Permission Check', () => {
    it('renders children when user has required permission', () => {
      authState = createAuthState({
        isAuthenticated: true,
        hasPermission: () => true,
      });

      renderProtectedRoute(
        <ProtectedRoute requiredPermission="chats:delete" useAuthHook={useAuthHook}>
          <ProtectedContent />
        </ProtectedRoute>
      );

      expect(screen.getByTestId('protected-content')).toBeInTheDocument();
    });

    it('redirects to unauthorized when user lacks required permission', () => {
      authState = createAuthState({
        isAuthenticated: true,
        hasPermission: () => false,
      });

      renderProtectedRoute(
        <ProtectedRoute requiredPermission="chats:delete" useAuthHook={useAuthHook}>
          <ProtectedContent />
        </ProtectedRoute>
      );

      expect(mockCurrentRoute).toBe('/unauthorized');
      expect(screen.queryByTestId('protected-content')).not.toBeInTheDocument();
    });
  });

  describe('Multiple Permissions Check', () => {
    it('renders children when user has any required permissions', () => {
      authState = createAuthState({
        isAuthenticated: true,
        hasAnyPermission: () => true,
      });

      renderProtectedRoute(
        <ProtectedRoute
          requiredPermissions={['chats:delete', 'chats:update']}
          useAuthHook={useAuthHook}
        >
          <ProtectedContent />
        </ProtectedRoute>
      );

      expect(screen.getByTestId('protected-content')).toBeInTheDocument();
    });

    it('redirects to unauthorized when user lacks all required permissions', () => {
      authState = createAuthState({
        isAuthenticated: true,
        hasAnyPermission: () => false,
      });

      renderProtectedRoute(
        <ProtectedRoute
          requiredPermissions={['chats:delete', 'chats:update']}
          useAuthHook={useAuthHook}
        >
          <ProtectedContent />
        </ProtectedRoute>
      );

      expect(mockCurrentRoute).toBe('/unauthorized');
      expect(screen.queryByTestId('protected-content')).not.toBeInTheDocument();
    });

    it('renders children when user has all required permissions', () => {
      authState = createAuthState({
        isAuthenticated: true,
        hasAllPermissions: () => true,
      });

      renderProtectedRoute(
        <ProtectedRoute
          requiredPermissions={['chats:delete', 'chats:update']}
          requireAll
          useAuthHook={useAuthHook}
        >
          <ProtectedContent />
        </ProtectedRoute>
      );

      expect(screen.getByTestId('protected-content')).toBeInTheDocument();
    });

    it('redirects to unauthorized when user lacks any required permission', () => {
      authState = createAuthState({
        isAuthenticated: true,
        hasAllPermissions: () => false,
      });

      renderProtectedRoute(
        <ProtectedRoute
          requiredPermissions={['chats:delete', 'chats:update']}
          requireAll
          useAuthHook={useAuthHook}
        >
          <ProtectedContent />
        </ProtectedRoute>
      );

      expect(mockCurrentRoute).toBe('/unauthorized');
      expect(screen.queryByTestId('protected-content')).not.toBeInTheDocument();
    });
  });
});
