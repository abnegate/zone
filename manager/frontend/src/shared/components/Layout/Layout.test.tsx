import { render, screen } from '@testing-library/react';
import { afterAll, beforeAll, describe, expect, it, mock } from 'bun:test';

// Mock Sidebar to avoid its dependencies
mock.module('../Sidebar/Sidebar', () => ({
  default: function MockSidebar() {
    return <aside data-testid="sidebar">Sidebar</aside>;
  },
}));

// Mock react-router-dom
mock.module('react-router-dom', () => ({
  Outlet: () => <div data-testid="outlet">Outlet content</div>,
  useLocation: () => ({ pathname: '/', search: '', hash: '', state: null, key: 'default' }),
  useNavigate: () => mock(),
  useSearchParams: () => [new URLSearchParams(), mock()],
  BrowserRouter: ({ children }: { children: React.ReactNode }) => children,
  Link: ({ children, to }: { children: React.ReactNode; to: string }) => <a href={to}>{children}</a>,
  NavLink: ({ children, to }: { children: React.ReactNode; to: string }) => <a href={to}>{children}</a>,
}));

// Mock auth context
mock.module('../../../features/auth/context', () => ({
  useAuth: () => ({
    isAuthenticated: true,
    user: { id: '1', email: 'test@test.com' },
    roles: ['user'],
    permissions: ['models:read'],
    hasPermission: () => true,
    hasAnyPermission: () => true,
    hasRole: () => true,
    logout: mock(),
    login: mock(),
  }),
  AuthProvider: ({ children }: { children: React.ReactNode }) => children,
}));

let Layout: typeof import('./Layout').default;

beforeAll(async () => {
  Layout = (await import('./Layout')).default;
});

afterAll(() => {
  mock.restore();
});

describe('Layout', () => {
  it('renders sidebar', () => {
    render(<Layout />);
    expect(screen.getByTestId('sidebar')).toBeInTheDocument();
  });

  it('renders outlet', () => {
    render(<Layout />);
    expect(screen.getByTestId('outlet')).toBeInTheDocument();
  });

  it('has main content area', () => {
    render(<Layout />);
    expect(document.querySelector('.main-content')).toBeInTheDocument();
  });

  it('has layout class', () => {
    render(<Layout />);
    expect(document.querySelector('.layout')).toBeInTheDocument();
  });
});
