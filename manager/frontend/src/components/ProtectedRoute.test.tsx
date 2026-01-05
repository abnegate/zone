import { render, screen } from '@testing-library/react';
import { useAuth } from '../context/AuthContext';
import ProtectedRoute from './ProtectedRoute';

// Mock react-router-dom
let mockCurrentRoute = '/';
const mockNavigate = jest.fn();
jest.mock('react-router-dom', () => ({
  Navigate: ({ to }: { to: string; replace?: boolean }) => {
    mockCurrentRoute = to;
    return null;
  },
  useNavigate: () => mockNavigate,
  useLocation: () => ({ pathname: mockCurrentRoute, state: null }),
}));

// Mock the AuthContext
jest.mock('../context/AuthContext');
const mockUseAuth = useAuth as jest.MockedFunction<typeof useAuth>;

// Test components
const ProtectedContent = () => <div data-testid="protected-content">Protected Content</div>;

// Helper to render
const renderProtectedRoute = (ui: React.ReactElement) => {
  mockCurrentRoute = '/';
  return render(ui);
};

describe('ProtectedRoute', () => {
  beforeEach(() => {
    jest.clearAllMocks();
    mockCurrentRoute = '/';
  });

  describe('Authentication', () => {
    it('shows loading spinner while auth is loading', () => {
      mockUseAuth.mockReturnValue({
        isAuthenticated: false,
        isLoading: true,
        user: null,
        accessToken: null,
        refreshToken: null,
        roles: [],
        permissions: [],
        login: jest.fn(),
        register: jest.fn(),
        logout: jest.fn(),
        hasPermission: jest.fn(),
        hasAnyPermission: jest.fn(),
        hasAllPermissions: jest.fn(),
        hasRole: jest.fn(),
      });

      renderProtectedRoute(
        <ProtectedRoute>
          <ProtectedContent />
        </ProtectedRoute>
      );

      expect(screen.getByText('Loading...')).toBeInTheDocument();
      expect(screen.queryByTestId('protected-content')).not.toBeInTheDocument();
    });

    it('redirects to login when not authenticated', () => {
      mockUseAuth.mockReturnValue({
        isAuthenticated: false,
        isLoading: false,
        user: null,
        accessToken: null,
        refreshToken: null,
        roles: [],
        permissions: [],
        login: jest.fn(),
        register: jest.fn(),
        logout: jest.fn(),
        hasPermission: jest.fn(),
        hasAnyPermission: jest.fn(),
        hasAllPermissions: jest.fn(),
        hasRole: jest.fn(),
      });

      renderProtectedRoute(
        <ProtectedRoute>
          <ProtectedContent />
        </ProtectedRoute>
      );

      expect(mockCurrentRoute).toBe('/login');
      expect(screen.queryByTestId('protected-content')).not.toBeInTheDocument();
    });

    it('renders children when authenticated', () => {
      mockUseAuth.mockReturnValue({
        isAuthenticated: true,
        isLoading: false,
        user: {
          id: '1',
          email: 'test@test.com',
          display_name: 'Test',
          is_admin: false,
          is_active: true,
          created_at: '2024-01-01T00:00:00Z',
          updated_at: '2024-01-01T00:00:00Z',
          last_login_at: null,
        },
        accessToken: 'token',
        refreshToken: 'refresh',
        roles: ['user'],
        permissions: [],
        login: jest.fn(),
        register: jest.fn(),
        logout: jest.fn(),
        hasPermission: jest.fn(),
        hasAnyPermission: jest.fn(),
        hasAllPermissions: jest.fn(),
        hasRole: jest.fn(),
      });

      renderProtectedRoute(
        <ProtectedRoute>
          <ProtectedContent />
        </ProtectedRoute>
      );

      expect(screen.getByTestId('protected-content')).toBeInTheDocument();
    });
  });

  describe('Single Permission Check', () => {
    it('renders children when user has required permission', () => {
      const mockHasPermission = jest.fn().mockReturnValue(true);
      mockUseAuth.mockReturnValue({
        isAuthenticated: true,
        isLoading: false,
        user: {
          id: '1',
          email: 'test@test.com',
          display_name: 'Test',
          is_admin: false,
          is_active: true,
          created_at: '2024-01-01T00:00:00Z',
          updated_at: '2024-01-01T00:00:00Z',
          last_login_at: null,
        },
        accessToken: 'token',
        refreshToken: 'refresh',
        roles: ['user'],
        permissions: ['chats:read'],
        login: jest.fn(),
        register: jest.fn(),
        logout: jest.fn(),
        hasPermission: mockHasPermission,
        hasAnyPermission: jest.fn(),
        hasAllPermissions: jest.fn(),
        hasRole: jest.fn(),
      });

      renderProtectedRoute(
        <ProtectedRoute requiredPermission="chats:read">
          <ProtectedContent />
        </ProtectedRoute>
      );

      expect(mockHasPermission).toHaveBeenCalledWith('chats:read');
      expect(screen.getByTestId('protected-content')).toBeInTheDocument();
    });

    it('redirects to unauthorized when user lacks required permission', () => {
      const mockHasPermission = jest.fn().mockReturnValue(false);
      mockUseAuth.mockReturnValue({
        isAuthenticated: true,
        isLoading: false,
        user: {
          id: '1',
          email: 'test@test.com',
          display_name: 'Test',
          is_admin: false,
          is_active: true,
          created_at: '2024-01-01T00:00:00Z',
          updated_at: '2024-01-01T00:00:00Z',
          last_login_at: null,
        },
        accessToken: 'token',
        refreshToken: 'refresh',
        roles: ['user'],
        permissions: [],
        login: jest.fn(),
        register: jest.fn(),
        logout: jest.fn(),
        hasPermission: mockHasPermission,
        hasAnyPermission: jest.fn(),
        hasAllPermissions: jest.fn(),
        hasRole: jest.fn(),
      });

      renderProtectedRoute(
        <ProtectedRoute requiredPermission="users:delete">
          <ProtectedContent />
        </ProtectedRoute>
      );

      expect(mockHasPermission).toHaveBeenCalledWith('users:delete');
      expect(mockCurrentRoute).toBe('/unauthorized');
      expect(screen.queryByTestId('protected-content')).not.toBeInTheDocument();
    });
  });

  describe('Multiple Permissions Check', () => {
    it('renders children when user has any of required permissions (requireAll=false)', () => {
      const mockHasAnyPermission = jest.fn().mockReturnValue(true);
      mockUseAuth.mockReturnValue({
        isAuthenticated: true,
        isLoading: false,
        user: {
          id: '1',
          email: 'test@test.com',
          display_name: 'Test',
          is_admin: false,
          is_active: true,
          created_at: '2024-01-01T00:00:00Z',
          updated_at: '2024-01-01T00:00:00Z',
          last_login_at: null,
        },
        accessToken: 'token',
        refreshToken: 'refresh',
        roles: ['user'],
        permissions: ['chats:read'],
        login: jest.fn(),
        register: jest.fn(),
        logout: jest.fn(),
        hasPermission: jest.fn(),
        hasAnyPermission: mockHasAnyPermission,
        hasAllPermissions: jest.fn(),
        hasRole: jest.fn(),
      });

      renderProtectedRoute(
        <ProtectedRoute requiredPermissions={['chats:read', 'chats:create']}>
          <ProtectedContent />
        </ProtectedRoute>
      );

      expect(mockHasAnyPermission).toHaveBeenCalledWith(['chats:read', 'chats:create']);
      expect(screen.getByTestId('protected-content')).toBeInTheDocument();
    });

    it('renders children when user has all required permissions (requireAll=true)', () => {
      const mockHasAllPermissions = jest.fn().mockReturnValue(true);
      mockUseAuth.mockReturnValue({
        isAuthenticated: true,
        isLoading: false,
        user: {
          id: '1',
          email: 'test@test.com',
          display_name: 'Test',
          is_admin: false,
          is_active: true,
          created_at: '2024-01-01T00:00:00Z',
          updated_at: '2024-01-01T00:00:00Z',
          last_login_at: null,
        },
        accessToken: 'token',
        refreshToken: 'refresh',
        roles: ['user'],
        permissions: ['chats:read', 'chats:create'],
        login: jest.fn(),
        register: jest.fn(),
        logout: jest.fn(),
        hasPermission: jest.fn(),
        hasAnyPermission: jest.fn(),
        hasAllPermissions: mockHasAllPermissions,
        hasRole: jest.fn(),
      });

      renderProtectedRoute(
        <ProtectedRoute requiredPermissions={['chats:read', 'chats:create']} requireAll>
          <ProtectedContent />
        </ProtectedRoute>
      );

      expect(mockHasAllPermissions).toHaveBeenCalledWith(['chats:read', 'chats:create']);
      expect(screen.getByTestId('protected-content')).toBeInTheDocument();
    });

    it('redirects to unauthorized when requireAll=true and user lacks one permission', () => {
      const mockHasAllPermissions = jest.fn().mockReturnValue(false);
      mockUseAuth.mockReturnValue({
        isAuthenticated: true,
        isLoading: false,
        user: {
          id: '1',
          email: 'test@test.com',
          display_name: 'Test',
          is_admin: false,
          is_active: true,
          created_at: '2024-01-01T00:00:00Z',
          updated_at: '2024-01-01T00:00:00Z',
          last_login_at: null,
        },
        accessToken: 'token',
        refreshToken: 'refresh',
        roles: ['user'],
        permissions: ['chats:read'],
        login: jest.fn(),
        register: jest.fn(),
        logout: jest.fn(),
        hasPermission: jest.fn(),
        hasAnyPermission: jest.fn(),
        hasAllPermissions: mockHasAllPermissions,
        hasRole: jest.fn(),
      });

      renderProtectedRoute(
        <ProtectedRoute requiredPermissions={['chats:read', 'chats:delete']} requireAll>
          <ProtectedContent />
        </ProtectedRoute>
      );

      expect(mockCurrentRoute).toBe('/unauthorized');
    });

    it('redirects to unauthorized when user has none of required permissions', () => {
      const mockHasAnyPermission = jest.fn().mockReturnValue(false);
      mockUseAuth.mockReturnValue({
        isAuthenticated: true,
        isLoading: false,
        user: {
          id: '1',
          email: 'test@test.com',
          display_name: 'Test',
          is_admin: false,
          is_active: true,
          created_at: '2024-01-01T00:00:00Z',
          updated_at: '2024-01-01T00:00:00Z',
          last_login_at: null,
        },
        accessToken: 'token',
        refreshToken: 'refresh',
        roles: ['user'],
        permissions: [],
        login: jest.fn(),
        register: jest.fn(),
        logout: jest.fn(),
        hasPermission: jest.fn(),
        hasAnyPermission: mockHasAnyPermission,
        hasAllPermissions: jest.fn(),
        hasRole: jest.fn(),
      });

      renderProtectedRoute(
        <ProtectedRoute requiredPermissions={['admin:read', 'admin:write']}>
          <ProtectedContent />
        </ProtectedRoute>
      );

      expect(mockCurrentRoute).toBe('/unauthorized');
    });
  });

  describe('Edge Cases', () => {
    it('handles empty permissions array gracefully', () => {
      mockUseAuth.mockReturnValue({
        isAuthenticated: true,
        isLoading: false,
        user: {
          id: '1',
          email: 'test@test.com',
          display_name: 'Test',
          is_admin: false,
          is_active: true,
          created_at: '2024-01-01T00:00:00Z',
          updated_at: '2024-01-01T00:00:00Z',
          last_login_at: null,
        },
        accessToken: 'token',
        refreshToken: 'refresh',
        roles: ['user'],
        permissions: [],
        login: jest.fn(),
        register: jest.fn(),
        logout: jest.fn(),
        hasPermission: jest.fn(),
        hasAnyPermission: jest.fn(),
        hasAllPermissions: jest.fn(),
        hasRole: jest.fn(),
      });

      renderProtectedRoute(
        <ProtectedRoute requiredPermissions={[]}>
          <ProtectedContent />
        </ProtectedRoute>
      );

      // Empty array should be treated as no permission requirement
      expect(screen.getByTestId('protected-content')).toBeInTheDocument();
    });

    it('prioritizes authentication check over permission check', () => {
      mockUseAuth.mockReturnValue({
        isAuthenticated: false,
        isLoading: false,
        user: null,
        accessToken: null,
        refreshToken: null,
        roles: [],
        permissions: [],
        login: jest.fn(),
        register: jest.fn(),
        logout: jest.fn(),
        hasPermission: jest.fn(),
        hasAnyPermission: jest.fn(),
        hasAllPermissions: jest.fn(),
        hasRole: jest.fn(),
      });

      renderProtectedRoute(
        <ProtectedRoute requiredPermission="chats:read">
          <ProtectedContent />
        </ProtectedRoute>
      );

      // Should redirect to login, not unauthorized
      expect(mockCurrentRoute).toBe('/login');
    });
  });
});
