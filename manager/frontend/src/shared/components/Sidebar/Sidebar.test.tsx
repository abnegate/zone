import { afterAll, beforeAll, beforeEach, describe, expect, it, mock } from 'bun:test';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';

mock.module('react-router-dom', () => ({
  BrowserRouter: ({ children }: { children: React.ReactNode }) => <>{children}</>,
  useLocation: () => ({ pathname: '/' }),
  useNavigate: () => mock(),
  useSearchParams: () => [new URLSearchParams(), mock()],
  Link: ({ to, children }: { to: string; children: React.ReactNode }) => (
    <a href={to}>{children}</a>
  ),
  NavLink: ({
    to,
    children,
    onClick,
    className,
    title,
  }: {
    to: string;
    children: React.ReactNode;
    onClick?: React.MouseEventHandler<HTMLAnchorElement>;
    className?: string | ((args: { isActive: boolean }) => string);
    title?: string;
  }) => {
    const resolvedClassName =
      typeof className === 'function' ? className({ isActive: false }) : className;
    return (
      <a href={to} onClick={onClick} className={resolvedClassName} title={title}>
        {children}
      </a>
    );
  },
}));

// Mock the client
mock.module('../../../api/client', () => ({
  client: {
    getOrganizations: mock(() => Promise.resolve([])),
    getWorkspaces: mock(() => Promise.resolve([])),
    setAccessToken: mock(),
  },
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

// Mock workspace context
mock.module('../../context/WorkspaceContext', () => ({
  useWorkspace: () => ({
    currentWorkspace: { id: 'test-ws', name: 'Test Workspace' },
    currentOrganization: { id: 'test-org', name: 'Test Org' },
    workspaces: [],
    organizations: [],
    loading: false,
    error: null,
    setCurrentWorkspace: mock(),
    setCurrentOrganization: mock(),
    refreshWorkspaces: mock(),
    refreshOrganizations: mock(),
  }),
  WorkspaceProvider: ({ children }: { children: React.ReactNode }) => children,
}));

// Mock theme context - reads theme from localStorage to support dynamic testing
mock.module('../../context/ThemeContext', () => ({
  useTheme: () => ({
    theme: localStorage.getItem('manager_theme') || 'light',
    setTheme: (t: string) => {
      localStorage.setItem('manager_theme', t);
    },
    toggleTheme: () => {
      const current = localStorage.getItem('manager_theme') || 'light';
      localStorage.setItem('manager_theme', current === 'light' ? 'dark' : 'light');
    },
  }),
  ThemeProvider: ({ children }: { children: React.ReactNode }) => children,
}));

let Sidebar: typeof import('./Sidebar').default;

beforeAll(async () => {
  Sidebar = (await import('./Sidebar')).default;
});

afterAll(() => {
  mock.restore();
});

const createWrapper = () => {
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: { retry: false, gcTime: 0 },
      mutations: { retry: false, gcTime: 0 },
    },
  });
  return ({ children }: { children: React.ReactNode }) => (
    <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
  );
};

const renderSidebar = () => {
  const Wrapper = createWrapper();
  return render(
    <Wrapper>
      <Sidebar />
    </Wrapper>
  );
};

describe('Sidebar', () => {
  beforeEach(() => {
    localStorage.clear();
    localStorage.setItem('manager_theme', 'light');
    document.documentElement.removeAttribute('data-sidebar-collapsed');
  });

  describe('rendering', () => {
    it('hides Change Server unless the Zone client is serving the UI', () => {
      renderSidebar();
      expect(screen.queryByText('Change Server')).not.toBeInTheDocument();
    });

    it('shows Change Server when the Zone client reports itself', async () => {
      const originalFetch = globalThis.fetch;
      globalThis.fetch = (async (input: RequestInfo | URL) => {
        if (String(input).includes('/__zone/info')) {
          return new Response(JSON.stringify({ client: true }), {
            status: 200,
            headers: { 'content-type': 'application/json' },
          });
        }
        return new Response(null, { status: 404 });
      }) as typeof fetch;

      try {
        renderSidebar();
        expect(await screen.findByText('Change Server')).toBeInTheDocument();
        expect(screen.getByText('Change Server').closest('a')).toHaveAttribute(
          'href',
          '/__zone/change-server'
        );
      } finally {
        globalThis.fetch = originalFetch;
      }
    });

    it('hides Change Server when the Zone client reports client:false', async () => {
      const originalFetch = globalThis.fetch;
      let resolved = false;
      globalThis.fetch = (async (input: RequestInfo | URL) => {
        if (String(input).includes('/__zone/info')) {
          resolved = true;
          return new Response(JSON.stringify({ client: false, host: 'https://zone.example.com' }), {
            status: 200,
            headers: { 'content-type': 'application/json' },
          });
        }
        return new Response(null, { status: 404 });
      }) as typeof fetch;

      try {
        renderSidebar();
        await waitFor(() => {
          expect(resolved).toBe(true);
        });
        expect(screen.queryByText('Change Server')).not.toBeInTheDocument();
      } finally {
        globalThis.fetch = originalFetch;
      }
    });

    it('hides Change Server when /__zone/info is missing', async () => {
      const originalFetch = globalThis.fetch;
      let resolved = false;
      globalThis.fetch = (async (input: RequestInfo | URL) => {
        if (String(input).includes('/__zone/info')) {
          resolved = true;
          return new Response(null, { status: 404 });
        }
        return new Response(null, { status: 404 });
      }) as typeof fetch;

      try {
        renderSidebar();
        await waitFor(() => {
          expect(resolved).toBe(true);
        });
        expect(screen.queryByText('Change Server')).not.toBeInTheDocument();
      } finally {
        globalThis.fetch = originalFetch;
      }
    });

    it('hides Change Server when /__zone/info fails', async () => {
      const originalFetch = globalThis.fetch;
      let resolved = false;
      globalThis.fetch = (async (input: RequestInfo | URL) => {
        if (String(input).includes('/__zone/info')) {
          resolved = true;
          throw new TypeError('Failed to fetch');
        }
        return new Response(null, { status: 404 });
      }) as typeof fetch;

      try {
        renderSidebar();
        await waitFor(() => {
          expect(resolved).toBe(true);
        });
        expect(screen.queryByText('Change Server')).not.toBeInTheDocument();
      } finally {
        globalThis.fetch = originalFetch;
      }
    });

    it('keeps Change Server available when the sidebar is collapsed', async () => {
      const originalFetch = globalThis.fetch;
      globalThis.fetch = (async (input: RequestInfo | URL) => {
        if (String(input).includes('/__zone/info')) {
          return new Response(JSON.stringify({ client: true }), {
            status: 200,
            headers: { 'content-type': 'application/json' },
          });
        }
        return new Response(null, { status: 404 });
      }) as typeof fetch;

      try {
        renderSidebar();
        expect(await screen.findByText('Change Server')).toBeInTheDocument();
        fireEvent.click(screen.getByLabelText('Collapse sidebar'));
        await waitFor(() => {
          expect(screen.queryByText('Change Server')).not.toBeInTheDocument();
        });
        expect(screen.getByTitle('Change Server')).toHaveAttribute('href', '/__zone/change-server');
      } finally {
        globalThis.fetch = originalFetch;
      }
    });

    it('renders logo', () => {
      renderSidebar();
      expect(screen.getByText('Zone')).toBeInTheDocument();
    });

    it('renders all navigation items', () => {
      renderSidebar();

      expect(screen.getByText('Chats')).toBeInTheDocument();
      expect(screen.getByText('Projects')).toBeInTheDocument();
      expect(screen.getByText('Tasks')).toBeInTheDocument();
      expect(screen.getByText('Sources')).toBeInTheDocument();
      expect(screen.getByText('Search')).toBeInTheDocument();
      expect(screen.getByText('Models')).toBeInTheDocument();
      expect(screen.getByText('Wiki')).toBeInTheDocument();
      expect(screen.getByText('Organization')).toBeInTheDocument();
      expect(screen.getByText('Workspace')).toBeInTheDocument();
    });

    it('renders theme toggle button', () => {
      renderSidebar();
      expect(screen.getByLabelText(/Switch to .* mode/)).toBeInTheDocument();
    });

    it('renders logout button', () => {
      renderSidebar();
      expect(screen.getByText('Logout')).toBeInTheDocument();
    });

    it('renders collapse button', () => {
      renderSidebar();
      expect(screen.getByLabelText('Collapse sidebar')).toBeInTheDocument();
    });

    it('renders context switcher', () => {
      renderSidebar();
      expect(screen.getByText('Test Org')).toBeInTheDocument();
      expect(screen.getByText('Test Workspace')).toBeInTheDocument();
    });

    it('renders mobile menu button', () => {
      renderSidebar();
      expect(screen.getByLabelText('Toggle menu')).toBeInTheDocument();
    });
  });

  describe('navigation', () => {
    it('has correct links', () => {
      renderSidebar();

      expect(screen.getByText('Chats').closest('a')).toHaveAttribute('href', '/chats');
      expect(screen.getByText('Projects').closest('a')).toHaveAttribute('href', '/projects');
      expect(screen.getByText('Tasks').closest('a')).toHaveAttribute('href', '/tasks');
      expect(screen.getByText('Sources').closest('a')).toHaveAttribute('href', '/sources');
      expect(screen.getByText('Search').closest('a')).toHaveAttribute('href', '/search');
      expect(screen.getByText('Models').closest('a')).toHaveAttribute('href', '/models');
      expect(screen.getByText('Wiki').closest('a')).toHaveAttribute('href', '/wiki');
      expect(screen.getByText('Organization').closest('a')).toHaveAttribute(
        'href',
        '/org-settings'
      );
      expect(screen.getByText('Workspace').closest('a')).toHaveAttribute('href', '/settings');
    });
  });

  describe('theme toggle', () => {
    it('toggles theme from light to dark', async () => {
      localStorage.setItem('manager_theme', 'light');

      renderSidebar();

      const themeButton = screen.getByLabelText('Switch to dark mode');
      fireEvent.click(themeButton);

      await waitFor(() => {
        expect(localStorage.getItem('manager_theme')).toBe('dark');
      });
    });

    it('toggles theme from dark to light', async () => {
      localStorage.setItem('manager_theme', 'dark');

      renderSidebar();

      const themeButton = screen.getByLabelText('Switch to light mode');
      fireEvent.click(themeButton);

      await waitFor(() => {
        expect(localStorage.getItem('manager_theme')).toBe('light');
      });
    });
  });

  describe('collapse functionality', () => {
    it('collapses sidebar', async () => {
      renderSidebar();

      const collapseButton = screen.getByLabelText('Collapse sidebar');
      fireEvent.click(collapseButton);

      await waitFor(() => {
        expect(localStorage.getItem('manager_sidebar_collapsed')).toBe('true');
      });
      expect(document.documentElement.getAttribute('data-sidebar-collapsed')).toBe('true');
    });

    it('expands collapsed sidebar', async () => {
      localStorage.setItem('manager_sidebar_collapsed', 'true');

      renderSidebar();

      const expandButton = screen.getByLabelText('Expand sidebar');
      fireEvent.click(expandButton);

      await waitFor(() => {
        expect(localStorage.getItem('manager_sidebar_collapsed')).toBe('false');
      });
    });

    it('restores collapsed state from localStorage', () => {
      localStorage.setItem('manager_sidebar_collapsed', 'true');

      renderSidebar();

      expect(screen.getByLabelText('Expand sidebar')).toBeInTheDocument();
    });

    it('hides text labels when collapsed', async () => {
      renderSidebar();

      fireEvent.click(screen.getByLabelText('Collapse sidebar'));

      await waitFor(() => {
        // Logo should be hidden
        expect(screen.queryByText('Zone')).not.toBeInTheDocument();
      });
    });
  });

  describe('mobile menu', () => {
    it('toggles mobile menu open', async () => {
      renderSidebar();

      const menuButton = screen.getByLabelText('Toggle menu');
      fireEvent.click(menuButton);

      await waitFor(() => {
        expect(screen.getByRole('button', { name: 'Close menu' })).toBeInTheDocument();
      });
    });

    it('closes mobile menu when overlay clicked', async () => {
      renderSidebar();

      fireEvent.click(screen.getByLabelText('Toggle menu'));

      await waitFor(() => {
        expect(screen.getByRole('button', { name: 'Close menu' })).toBeInTheDocument();
      });

      const overlay = screen.getByRole('button', { name: 'Close menu' });
      fireEvent.click(overlay);

      await waitFor(() => {
        expect(screen.queryByRole('button', { name: 'Close menu' })).not.toBeInTheDocument();
      });
    });

    it('closes mobile menu when nav item clicked', async () => {
      renderSidebar();

      fireEvent.click(screen.getByLabelText('Toggle menu'));

      await waitFor(() => {
        expect(screen.getByRole('button', { name: 'Close menu' })).toBeInTheDocument();
      });

      fireEvent.click(screen.getByRole('link', { name: 'Chats' }));

      await waitFor(() => {
        expect(screen.queryByRole('button', { name: 'Close menu' })).not.toBeInTheDocument();
      });
    });

    it('closes mobile menu when Escape key is pressed', async () => {
      renderSidebar();

      fireEvent.click(screen.getByLabelText('Toggle menu'));

      await waitFor(() => {
        expect(screen.getByRole('button', { name: 'Close menu' })).toBeInTheDocument();
      });

      const overlay = screen.getByRole('button', { name: 'Close menu' });
      fireEvent.keyDown(overlay, { key: 'Escape' });

      await waitFor(() => {
        expect(screen.queryByRole('button', { name: 'Close menu' })).not.toBeInTheDocument();
      });
    });

    it('does not close mobile menu on other key press', async () => {
      renderSidebar();

      fireEvent.click(screen.getByLabelText('Toggle menu'));

      await waitFor(() => {
        expect(screen.getByRole('button', { name: 'Close menu' })).toBeInTheDocument();
      });

      const overlay = screen.getByRole('button', { name: 'Close menu' });
      fireEvent.keyDown(overlay, { key: 'Enter' });

      // Should still be open
      expect(screen.getByRole('button', { name: 'Close menu' })).toBeInTheDocument();
    });
  });

  describe('logout', () => {
    it('has a logout button that can be clicked', () => {
      renderSidebar();

      const logoutButton = screen.getByText('Logout');
      expect(logoutButton).toBeInTheDocument();

      // Just verify the button is clickable - actual logout behavior tested in AuthContext
      fireEvent.click(logoutButton);
    });
  });
});
