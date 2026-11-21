import { afterAll, beforeAll, beforeEach, describe, expect, it, mock } from 'bun:test';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { render as renderElement, screen } from '@testing-library/react';
import type { ReactElement } from 'react';
import { COMMUNITY, COMMUNITY_OVER_CAP } from '../../../features/enterprise/fixtures';

const getLicense = mock();

mock.module('../../../features/enterprise/api', () => ({
  licenseApi: { get: getLicense, set: mock(), remove: mock() },
}));

let admin = false;

mock.module('../Sidebar/Sidebar', () => ({
  default: function MockSidebar() {
    return <aside data-testid="sidebar">Sidebar</aside>;
  },
}));

mock.module('../../../features/models', () => ({
  DownloadDock: () => null,
  TrainDock: () => null,
}));

mock.module('./usePreloadPages', () => ({
  usePreloadPages: () => {},
}));

mock.module('react-router-dom', () => ({
  Outlet: () => <div data-testid="outlet">Outlet content</div>,
  useLocation: () => ({ pathname: '/', search: '', hash: '', state: null, key: 'default' }),
  useNavigate: () => mock(),
  useSearchParams: () => [new URLSearchParams(), mock()],
  BrowserRouter: ({ children }: { children: React.ReactNode }) => children,
  Link: ({ children, to }: { children: React.ReactNode; to: string }) => (
    <a href={to}>{children}</a>
  ),
  NavLink: ({ children, to }: { children: React.ReactNode; to: string }) => (
    <a href={to}>{children}</a>
  ),
}));

mock.module('../../../features/auth/context', () => ({
  useAuth: () => ({
    isAuthenticated: true,
    user: { id: '1', email: 'test@test.com', is_admin: admin },
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

function render(element: ReactElement) {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false, gcTime: 0 } } });
  return renderElement(<QueryClientProvider client={queryClient}>{element}</QueryClientProvider>);
}

describe('Layout', () => {
  beforeEach(() => {
    admin = false;
    getLicense.mockReset();
    getLicense.mockResolvedValue(COMMUNITY);
  });

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

  it('shows a server admin the over-cap banner above the page', async () => {
    admin = true;
    getLicense.mockResolvedValue(COMMUNITY_OVER_CAP);
    render(<Layout />);

    const banner = await screen.findByRole('status');
    expect(banner).toHaveTextContent('Everyone keeps access');
    expect(banner.parentElement).toHaveClass('main-content');
    expect(banner.compareDocumentPosition(screen.getByTestId('outlet'))).toBe(
      Node.DOCUMENT_POSITION_FOLLOWING
    );
  });

  it('shows no banner to someone who is not a server admin', async () => {
    getLicense.mockResolvedValue(COMMUNITY_OVER_CAP);
    render(<Layout />);

    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(screen.queryByRole('status')).not.toBeInTheDocument();
    expect(getLicense).not.toHaveBeenCalled();
  });
});
