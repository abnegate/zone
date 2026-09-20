import { afterAll, beforeAll, beforeEach, describe, expect, it, mock } from 'bun:test';
import { render, screen } from '@testing-library/react';

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

  // Permissions are global to the account, so a member of one organization
  // holds organizations:update everywhere; the role held in the current
  // organization decides whether its settings open.
  describe('Organization role', () => {
    const organization = (role: 'owner' | 'admin' | 'member' | undefined) => ({
      id: 'org-1',
      name: 'Zone Verify',
      slug: 'zone-verify',
      description: null,
      is_active: true,
      role,
      created_at: '2026-01-01T00:00:00Z',
      updated_at: '2026-01-01T00:00:00Z',
    });
    const workspaceHook =
      (role: 'owner' | 'admin' | 'member' | undefined, loading = false) =>
      () =>
        ({
          organizations: [organization(role)],
          currentOrganization: organization(role),
          currentWorkspace: null,
          workspaces: [],
          loading,
          error: null,
          setCurrentOrganization: () => {},
          setCurrentWorkspace: () => {},
          refreshOrganizations: async () => {},
          refreshWorkspaces: async () => {},
        }) as ReturnType<typeof import('../context/WorkspaceContext').useWorkspace>;

    beforeEach(() => {
      authState = createAuthState({
        isAuthenticated: true,
        hasPermission: () => true,
      });
    });

    it('redirects a plain member of the current organization to unauthorized', () => {
      renderProtectedRoute(
        <ProtectedRoute
          requiredPermission="organizations:update"
          requiredOrganizationRole="admin"
          useAuthHook={useAuthHook}
          useWorkspaceHook={workspaceHook('member')}
        >
          <ProtectedContent />
        </ProtectedRoute>
      );

      expect(mockCurrentRoute).toBe('/unauthorized');
      expect(screen.queryByTestId('protected-content')).not.toBeInTheDocument();
    });

    it('renders children for an admin or owner of the current organization', () => {
      for (const role of ['admin', 'owner'] as const) {
        const { unmount } = renderProtectedRoute(
          <ProtectedRoute
            requiredPermission="organizations:update"
            requiredOrganizationRole="admin"
            useAuthHook={useAuthHook}
            useWorkspaceHook={workspaceHook(role)}
          >
            <ProtectedContent />
          </ProtectedRoute>
        );

        expect(screen.getByTestId('protected-content')).toBeInTheDocument();
        unmount();
      }
    });

    it('waits for the organizations to load before deciding', () => {
      renderProtectedRoute(
        <ProtectedRoute
          requiredOrganizationRole="admin"
          useAuthHook={useAuthHook}
          useWorkspaceHook={workspaceHook('member', true)}
        >
          <ProtectedContent />
        </ProtectedRoute>
      );

      expect(screen.getByText('Loading...')).toBeInTheDocument();
      expect(mockCurrentRoute).toBe('/');
    });

    it('ignores the organization role when none is required', () => {
      renderProtectedRoute(
        <ProtectedRoute useAuthHook={useAuthHook} useWorkspaceHook={workspaceHook('member')}>
          <ProtectedContent />
        </ProtectedRoute>
      );

      expect(screen.getByTestId('protected-content')).toBeInTheDocument();
    });
  });
});
