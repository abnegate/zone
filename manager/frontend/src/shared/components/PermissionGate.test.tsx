import { render, screen } from '@testing-library/react';
import { beforeEach, describe, expect, it } from 'bun:test';
import PermissionGate from './PermissionGate';

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
const ProtectedButton = () => <button data-testid="delete-button">Delete</button>;
const FallbackMessage = () => <span data-testid="fallback">You cannot delete</span>;

describe('PermissionGate', () => {
  beforeEach(() => {
    authState = createAuthState();
  });

  describe('Single Permission', () => {
    it('renders children when user has permission', () => {
      authState = createAuthState({
        permissions: ['chats:delete'],
        hasPermission: () => true,
      });

      render(
        <PermissionGate permission="chats:delete" useAuthHook={useAuthHook}>
          <ProtectedButton />
        </PermissionGate>
      );

      expect(screen.getByTestId('delete-button')).toBeInTheDocument();
    });

    it('renders nothing when user lacks permission and no fallback', () => {
      authState = createAuthState({
        hasPermission: () => false,
      });

      render(
        <PermissionGate permission="chats:delete" useAuthHook={useAuthHook}>
          <ProtectedButton />
        </PermissionGate>
      );

      expect(screen.queryByTestId('delete-button')).not.toBeInTheDocument();
    });

    it('renders fallback when user lacks permission', () => {
      authState = createAuthState({
        hasPermission: () => false,
      });

      render(
        <PermissionGate
          permission="chats:delete"
          fallback={<FallbackMessage />}
          useAuthHook={useAuthHook}
        >
          <ProtectedButton />
        </PermissionGate>
      );

      expect(screen.queryByTestId('delete-button')).not.toBeInTheDocument();
      expect(screen.getByTestId('fallback')).toBeInTheDocument();
    });
  });

  describe('Multiple Permissions (Any)', () => {
    it('renders children when user has any of the permissions', () => {
      authState = createAuthState({
        permissions: ['chats:update'],
        hasAnyPermission: () => true,
      });

      render(
        <PermissionGate
          permissions={['chats:update', 'chats:delete']}
          useAuthHook={useAuthHook}
        >
          <ProtectedButton />
        </PermissionGate>
      );

      expect(screen.getByTestId('delete-button')).toBeInTheDocument();
    });

    it('renders nothing when user has none of the permissions', () => {
      authState = createAuthState({
        hasAnyPermission: () => false,
      });

      render(
        <PermissionGate
          permissions={['chats:update', 'chats:delete']}
          useAuthHook={useAuthHook}
        >
          <ProtectedButton />
        </PermissionGate>
      );

      expect(screen.queryByTestId('delete-button')).not.toBeInTheDocument();
    });
  });

  describe('Multiple Permissions (All Required)', () => {
    it('renders children when user has all permissions', () => {
      authState = createAuthState({
        permissions: ['chats:update', 'chats:delete'],
        hasAllPermissions: () => true,
      });

      render(
        <PermissionGate
          permissions={['chats:update', 'chats:delete']}
          requireAll
          useAuthHook={useAuthHook}
        >
          <ProtectedButton />
        </PermissionGate>
      );

      expect(screen.getByTestId('delete-button')).toBeInTheDocument();
    });

    it('renders nothing when user lacks one of required permissions', () => {
      authState = createAuthState({
        permissions: ['chats:update'],
        hasAllPermissions: () => false,
      });

      render(
        <PermissionGate
          permissions={['chats:update', 'chats:delete']}
          requireAll
          useAuthHook={useAuthHook}
        >
          <ProtectedButton />
        </PermissionGate>
      );

      expect(screen.queryByTestId('delete-button')).not.toBeInTheDocument();
    });

    it('renders fallback when requireAll fails', () => {
      authState = createAuthState({
        hasAllPermissions: () => false,
      });

      render(
        <PermissionGate
          permissions={['admin:read', 'admin:write']}
          requireAll
          fallback={<FallbackMessage />}
          useAuthHook={useAuthHook}
        >
          <ProtectedButton />
        </PermissionGate>
      );

      expect(screen.getByTestId('fallback')).toBeInTheDocument();
    });
  });

  describe('No Permissions Specified', () => {
    it('renders children when no permission prop provided', () => {
      authState = createAuthState();

      render(
        <PermissionGate useAuthHook={useAuthHook}>
          <ProtectedButton />
        </PermissionGate>
      );

      expect(screen.getByTestId('delete-button')).toBeInTheDocument();
    });

    it('renders children when empty permissions array provided', () => {
      authState = createAuthState();

      render(
        <PermissionGate permissions={[]} useAuthHook={useAuthHook}>
          <ProtectedButton />
        </PermissionGate>
      );

      expect(screen.getByTestId('delete-button')).toBeInTheDocument();
    });
  });

  describe('Nested PermissionGates', () => {
    it('correctly handles nested gates', () => {
      authState = createAuthState({
        permissions: ['projects:read'],
        hasPermission: (perm) => perm === 'projects:read',
      });

      render(
        <PermissionGate permission="projects:read" useAuthHook={useAuthHook}>
          <div data-testid="outer">
            Outer Content
            <PermissionGate
              permission="projects:delete"
              fallback={<span data-testid="inner-fallback">No Delete</span>}
              useAuthHook={useAuthHook}
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
      authState = createAuthState({
        hasPermission: () => false,
      });

      render(
        <PermissionGate permission="admin:all" fallback="Access denied" useAuthHook={useAuthHook}>
          <ProtectedButton />
        </PermissionGate>
      );

      expect(screen.getByText('Access denied')).toBeInTheDocument();
    });

    it('accepts null as fallback (renders nothing)', () => {
      authState = createAuthState({
        hasPermission: () => false,
      });

      const { container } = render(
        <PermissionGate permission="admin:all" fallback={null} useAuthHook={useAuthHook}>
          <ProtectedButton />
        </PermissionGate>
      );

      expect(container.firstChild).toBeNull();
    });
  });
});
