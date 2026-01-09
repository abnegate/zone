import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { BrowserRouter } from 'react-router-dom';
import { AuthProvider } from '../../../features/auth';
import { ThemeProvider } from '../../context/ThemeContext';
import { WorkspaceProvider } from '../../context/WorkspaceContext';
import Sidebar from './Sidebar';

// Mock the client
jest.mock('../../../api/client', () => ({
  client: {
    getOrganizations: jest.fn().mockResolvedValue([]),
    getWorkspaces: jest.fn().mockResolvedValue([]),
    setAccessToken: jest.fn(),
  },
}));

// Mock ContextSwitcher to simplify tests
jest.mock('../ContextSwitcher/ContextSwitcher', () => {
  return function MockContextSwitcher() {
    return <div data-testid="context-switcher">Context Switcher</div>;
  };
});

const setupAuth = () => {
  localStorage.setItem('accessToken', 'test-token');
  localStorage.setItem('user', JSON.stringify({ id: '1', email: 'test@test.com' }));
  localStorage.setItem('roles', JSON.stringify(['user']));
  localStorage.setItem('permissions', JSON.stringify(['models:read']));
  // Always set a theme to avoid matchMedia calls
  localStorage.setItem('manager_theme', 'light');
};

const renderSidebar = () => {
  return render(
    <BrowserRouter>
      <AuthProvider>
        <ThemeProvider>
          <WorkspaceProvider>
            <Sidebar />
          </WorkspaceProvider>
        </ThemeProvider>
      </AuthProvider>
    </BrowserRouter>
  );
};

describe('Sidebar', () => {
  beforeEach(() => {
    localStorage.clear();
    setupAuth();
    document.documentElement.removeAttribute('data-sidebar-collapsed');
  });

  describe('rendering', () => {
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
      expect(screen.getByTestId('context-switcher')).toBeInTheDocument();
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
      expect(screen.getByText('Models').closest('a')).toHaveAttribute('href', '/');
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

      fireEvent.click(screen.getByText('Chats'));

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
