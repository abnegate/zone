import { render, screen } from '@testing-library/react';
import { useAuth } from '../context/AuthContext';
import PermissionGate from './PermissionGate';

// Mock the AuthContext
jest.mock('../context/AuthContext');
const mockUseAuth = useAuth as jest.MockedFunction<typeof useAuth>;

// Test components
const ProtectedButton = () => <button data-testid="delete-button">Delete</button>;
const FallbackMessage = () => <span data-testid="fallback">You cannot delete</span>;

describe('PermissionGate', () => {
  beforeEach(() => {
    jest.clearAllMocks();
  });

  describe('Single Permission', () => {
    it('renders children when user has permission', () => {
      mockUseAuth.mockReturnValue({
        isAuthenticated: true,
        isLoading: false,
        user: { id: '1', email: 'test@test.com', display_name: 'Test', is_admin: false },
        accessToken: 'token',
        refreshToken: 'refresh',
        roles: ['user'],
        permissions: ['chats:delete'],
        login: jest.fn(),
        register: jest.fn(),
        logout: jest.fn(),
        hasPermission: jest.fn().mockReturnValue(true),
        hasAnyPermission: jest.fn(),
        hasAllPermissions: jest.fn(),
        hasRole: jest.fn(),
      });

      render(
        <PermissionGate permission="chats:delete">
          <ProtectedButton />
        </PermissionGate>
      );

      expect(screen.getByTestId('delete-button')).toBeInTheDocument();
    });

    it('renders nothing when user lacks permission and no fallback', () => {
      mockUseAuth.mockReturnValue({
        isAuthenticated: true,
        isLoading: false,
        user: { id: '1', email: 'test@test.com', display_name: 'Test', is_admin: false },
        accessToken: 'token',
        refreshToken: 'refresh',
        roles: ['user'],
        permissions: [],
        login: jest.fn(),
        register: jest.fn(),
        logout: jest.fn(),
        hasPermission: jest.fn().mockReturnValue(false),
        hasAnyPermission: jest.fn(),
        hasAllPermissions: jest.fn(),
        hasRole: jest.fn(),
      });

      render(
        <PermissionGate permission="chats:delete">
          <ProtectedButton />
        </PermissionGate>
      );

      expect(screen.queryByTestId('delete-button')).not.toBeInTheDocument();
    });

    it('renders fallback when user lacks permission', () => {
      mockUseAuth.mockReturnValue({
        isAuthenticated: true,
        isLoading: false,
        user: { id: '1', email: 'test@test.com', display_name: 'Test', is_admin: false },
        accessToken: 'token',
        refreshToken: 'refresh',
        roles: ['user'],
        permissions: [],
        login: jest.fn(),
        register: jest.fn(),
        logout: jest.fn(),
        hasPermission: jest.fn().mockReturnValue(false),
        hasAnyPermission: jest.fn(),
        hasAllPermissions: jest.fn(),
        hasRole: jest.fn(),
      });

      render(
        <PermissionGate permission="chats:delete" fallback={<FallbackMessage />}>
          <ProtectedButton />
        </PermissionGate>
      );

      expect(screen.queryByTestId('delete-button')).not.toBeInTheDocument();
      expect(screen.getByTestId('fallback')).toBeInTheDocument();
    });
  });

  describe('Multiple Permissions (Any)', () => {
    it('renders children when user has any of the permissions', () => {
      mockUseAuth.mockReturnValue({
        isAuthenticated: true,
        isLoading: false,
        user: { id: '1', email: 'test@test.com', display_name: 'Test', is_admin: false },
        accessToken: 'token',
        refreshToken: 'refresh',
        roles: ['user'],
        permissions: ['chats:update'],
        login: jest.fn(),
        register: jest.fn(),
        logout: jest.fn(),
        hasPermission: jest.fn(),
        hasAnyPermission: jest.fn().mockReturnValue(true),
        hasAllPermissions: jest.fn(),
        hasRole: jest.fn(),
      });

      render(
        <PermissionGate permissions={['chats:update', 'chats:delete']}>
          <ProtectedButton />
        </PermissionGate>
      );

      expect(screen.getByTestId('delete-button')).toBeInTheDocument();
    });

    it('renders nothing when user has none of the permissions', () => {
      mockUseAuth.mockReturnValue({
        isAuthenticated: true,
        isLoading: false,
        user: { id: '1', email: 'test@test.com', display_name: 'Test', is_admin: false },
        accessToken: 'token',
        refreshToken: 'refresh',
        roles: ['user'],
        permissions: [],
        login: jest.fn(),
        register: jest.fn(),
        logout: jest.fn(),
        hasPermission: jest.fn(),
        hasAnyPermission: jest.fn().mockReturnValue(false),
        hasAllPermissions: jest.fn(),
        hasRole: jest.fn(),
      });

      render(
        <PermissionGate permissions={['chats:update', 'chats:delete']}>
          <ProtectedButton />
        </PermissionGate>
      );

      expect(screen.queryByTestId('delete-button')).not.toBeInTheDocument();
    });
  });

  describe('Multiple Permissions (All Required)', () => {
    it('renders children when user has all permissions', () => {
      mockUseAuth.mockReturnValue({
        isAuthenticated: true,
        isLoading: false,
        user: { id: '1', email: 'test@test.com', display_name: 'Test', is_admin: true },
        accessToken: 'token',
        refreshToken: 'refresh',
        roles: ['admin'],
        permissions: ['chats:update', 'chats:delete'],
        login: jest.fn(),
        register: jest.fn(),
        logout: jest.fn(),
        hasPermission: jest.fn(),
        hasAnyPermission: jest.fn(),
        hasAllPermissions: jest.fn().mockReturnValue(true),
        hasRole: jest.fn(),
      });

      render(
        <PermissionGate permissions={['chats:update', 'chats:delete']} requireAll>
          <ProtectedButton />
        </PermissionGate>
      );

      expect(screen.getByTestId('delete-button')).toBeInTheDocument();
    });

    it('renders nothing when user lacks one of required permissions', () => {
      mockUseAuth.mockReturnValue({
        isAuthenticated: true,
        isLoading: false,
        user: { id: '1', email: 'test@test.com', display_name: 'Test', is_admin: false },
        accessToken: 'token',
        refreshToken: 'refresh',
        roles: ['user'],
        permissions: ['chats:update'],
        login: jest.fn(),
        register: jest.fn(),
        logout: jest.fn(),
        hasPermission: jest.fn(),
        hasAnyPermission: jest.fn(),
        hasAllPermissions: jest.fn().mockReturnValue(false),
        hasRole: jest.fn(),
      });

      render(
        <PermissionGate permissions={['chats:update', 'chats:delete']} requireAll>
          <ProtectedButton />
        </PermissionGate>
      );

      expect(screen.queryByTestId('delete-button')).not.toBeInTheDocument();
    });

    it('renders fallback when requireAll fails', () => {
      mockUseAuth.mockReturnValue({
        isAuthenticated: true,
        isLoading: false,
        user: { id: '1', email: 'test@test.com', display_name: 'Test', is_admin: false },
        accessToken: 'token',
        refreshToken: 'refresh',
        roles: ['user'],
        permissions: [],
        login: jest.fn(),
        register: jest.fn(),
        logout: jest.fn(),
        hasPermission: jest.fn(),
        hasAnyPermission: jest.fn(),
        hasAllPermissions: jest.fn().mockReturnValue(false),
        hasRole: jest.fn(),
      });

      render(
        <PermissionGate
          permissions={['admin:read', 'admin:write']}
          requireAll
          fallback={<FallbackMessage />}
        >
          <ProtectedButton />
        </PermissionGate>
      );

      expect(screen.getByTestId('fallback')).toBeInTheDocument();
    });
  });

  describe('No Permissions Specified', () => {
    it('renders children when no permission prop provided', () => {
      mockUseAuth.mockReturnValue({
        isAuthenticated: true,
        isLoading: false,
        user: { id: '1', email: 'test@test.com', display_name: 'Test', is_admin: false },
        accessToken: 'token',
        refreshToken: 'refresh',
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

      render(
        <PermissionGate>
          <ProtectedButton />
        </PermissionGate>
      );

      expect(screen.getByTestId('delete-button')).toBeInTheDocument();
    });

    it('renders children when empty permissions array provided', () => {
      mockUseAuth.mockReturnValue({
        isAuthenticated: true,
        isLoading: false,
        user: { id: '1', email: 'test@test.com', display_name: 'Test', is_admin: false },
        accessToken: 'token',
        refreshToken: 'refresh',
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

      render(
        <PermissionGate permissions={[]}>
          <ProtectedButton />
        </PermissionGate>
      );

      expect(screen.getByTestId('delete-button')).toBeInTheDocument();
    });
  });

  describe('Nested PermissionGates', () => {
    it('correctly handles nested gates', () => {
      const hasPermission = jest.fn().mockImplementation((perm) => {
        return perm === 'projects:read';
      });

      mockUseAuth.mockReturnValue({
        isAuthenticated: true,
        isLoading: false,
        user: { id: '1', email: 'test@test.com', display_name: 'Test', is_admin: false },
        accessToken: 'token',
        refreshToken: 'refresh',
        roles: ['user'],
        permissions: ['projects:read'],
        login: jest.fn(),
        register: jest.fn(),
        logout: jest.fn(),
        hasPermission,
        hasAnyPermission: jest.fn(),
        hasAllPermissions: jest.fn(),
        hasRole: jest.fn(),
      });

      render(
        <PermissionGate permission="projects:read">
          <div data-testid="outer">
            Outer Content
            <PermissionGate
              permission="projects:delete"
              fallback={<span data-testid="inner-fallback">No Delete</span>}
            >
              <button data-testid="delete">Delete</button>
            </PermissionGate>
          </div>
        </PermissionGate>
      );

      expect(screen.getByTestId('outer')).toBeInTheDocument();
      expect(screen.getByTestId('inner-fallback')).toBeInTheDocument();
      expect(screen.queryByTestId('delete')).not.toBeInTheDocument();
    });
  });

  describe('Fallback Content Types', () => {
    it('accepts string as fallback', () => {
      mockUseAuth.mockReturnValue({
        isAuthenticated: true,
        isLoading: false,
        user: { id: '1', email: 'test@test.com', display_name: 'Test', is_admin: false },
        accessToken: 'token',
        refreshToken: 'refresh',
        roles: [],
        permissions: [],
        login: jest.fn(),
        register: jest.fn(),
        logout: jest.fn(),
        hasPermission: jest.fn().mockReturnValue(false),
        hasAnyPermission: jest.fn(),
        hasAllPermissions: jest.fn(),
        hasRole: jest.fn(),
      });

      render(
        <PermissionGate permission="admin:all" fallback="Access denied">
          <ProtectedButton />
        </PermissionGate>
      );

      expect(screen.getByText('Access denied')).toBeInTheDocument();
    });

    it('accepts null as fallback (renders nothing)', () => {
      mockUseAuth.mockReturnValue({
        isAuthenticated: true,
        isLoading: false,
        user: { id: '1', email: 'test@test.com', display_name: 'Test', is_admin: false },
        accessToken: 'token',
        refreshToken: 'refresh',
        roles: [],
        permissions: [],
        login: jest.fn(),
        register: jest.fn(),
        logout: jest.fn(),
        hasPermission: jest.fn().mockReturnValue(false),
        hasAnyPermission: jest.fn(),
        hasAllPermissions: jest.fn(),
        hasRole: jest.fn(),
      });

      const { container } = render(
        <PermissionGate permission="admin:all" fallback={null}>
          <ProtectedButton />
        </PermissionGate>
      );

      expect(container.firstChild).toBeNull();
    });
  });
});
