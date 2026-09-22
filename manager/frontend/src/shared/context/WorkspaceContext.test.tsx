import { afterAll, beforeAll, beforeEach, describe, expect, it, mock } from 'bun:test';
import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';
import type { Organization, Workspace } from '../../types';

const mockClient = {
  getOrganizations: mock(),
  getWorkspaces: mock(),
  getOrgMembers: mock(async () => ({ members: [] })),
};

let authState = {
  isAuthenticated: true,
  isLoading: false,
  user: { id: 'user-1', email: 'test@example.com' },
};
const useAuthHook = () => authState;

mock.module('../../api/client', () => ({
  client: mockClient,
}));

let WorkspaceProvider: typeof import('./WorkspaceContext').WorkspaceProvider;
let useWorkspace: typeof import('./WorkspaceContext').useWorkspace;

beforeAll(async () => {
  const ctx = await import('./WorkspaceContext');
  WorkspaceProvider = ctx.WorkspaceProvider;
  useWorkspace = ctx.useWorkspace;
});

afterAll(() => {
  mock.restore();
});

const mockOrganizations: Organization[] = [
  {
    id: 'org-1',
    name: 'Org 1',
    slug: 'org-1',
    description: null,
    is_active: true,
    created_at: '',
    updated_at: '',
  },
  {
    id: 'org-2',
    name: 'Org 2',
    slug: 'org-2',
    description: null,
    is_active: true,
    created_at: '',
    updated_at: '',
  },
];

const mockWorkspaces: Workspace[] = [
  {
    id: 'ws-1',
    organization_id: 'org-1',
    name: 'Workspace 1',
    slug: 'ws-1',
    description: null,
    is_active: true,
    created_at: '',
    updated_at: '',
  },
  {
    id: 'ws-2',
    organization_id: 'org-1',
    name: 'Workspace 2',
    slug: 'ws-2',
    description: null,
    is_active: true,
    created_at: '',
    updated_at: '',
  },
];

// Test component to access context
function TestComponent() {
  const ctx = useWorkspace();
  return (
    <div>
      <span data-testid="loading">{ctx.loading ? 'loading' : 'done'}</span>
      <span data-testid="error">{ctx.error || 'none'}</span>
      <span data-testid="orgs-count">{ctx.organizations.length}</span>
      <span data-testid="current-org">{ctx.currentOrganization?.name || 'none'}</span>
      <span data-testid="current-role">{ctx.currentOrganization?.role ?? 'unknown'}</span>
      <span data-testid="resolving-role">{ctx.resolvingRole ? 'resolving' : 'settled'}</span>
      <span data-testid="ws-count">{ctx.workspaces.length}</span>
      <span data-testid="current-ws">{ctx.currentWorkspace?.name || 'none'}</span>
      <button onClick={() => ctx.setCurrentOrganization(mockOrganizations[1])}>Switch Org</button>
      <button onClick={() => ctx.setCurrentWorkspace(mockWorkspaces[1])}>Switch Workspace</button>
      <button onClick={() => ctx.refreshOrganizations()}>Refresh Orgs</button>
      <button onClick={() => ctx.refreshWorkspaces()}>Refresh Workspaces</button>
    </div>
  );
}

describe('WorkspaceContext', () => {
  beforeEach(() => {
    mock.clearAllMocks();
    localStorage.clear();
    authState = {
      isAuthenticated: true,
      isLoading: false,
      user: { id: 'user-1', email: 'test@example.com' },
    };
  });

  describe('WorkspaceProvider', () => {
    it('loads organizations on mount', async () => {
      mockClient.getOrganizations.mockResolvedValueOnce(mockOrganizations);
      mockClient.getWorkspaces.mockResolvedValueOnce(mockWorkspaces);

      render(
        <WorkspaceProvider useAuthHook={useAuthHook}>
          <TestComponent />
        </WorkspaceProvider>
      );

      expect(screen.getByTestId('loading')).toHaveTextContent('loading');

      await waitFor(() => {
        expect(screen.getByTestId('loading')).toHaveTextContent('done');
      });

      expect(screen.getByTestId('orgs-count')).toHaveTextContent('2');
      expect(screen.getByTestId('current-org')).toHaveTextContent('Org 1');
    });

    it('stays loading until the picked organization has its workspaces', async () => {
      let resolveWorkspaces: (workspaces: Workspace[]) => void = () => undefined;
      mockClient.getOrganizations.mockResolvedValueOnce(mockOrganizations);
      mockClient.getWorkspaces.mockReturnValueOnce(
        new Promise<Workspace[]>((resolve) => {
          resolveWorkspaces = resolve;
        })
      );

      render(
        <WorkspaceProvider useAuthHook={useAuthHook}>
          <TestComponent />
        </WorkspaceProvider>
      );

      await waitFor(() => {
        expect(screen.getByTestId('current-org')).toHaveTextContent('Org 1');
      });
      expect(screen.getByTestId('loading')).toHaveTextContent('loading');
      expect(screen.getByTestId('current-ws')).toHaveTextContent('none');

      await act(async () => {
        resolveWorkspaces(mockWorkspaces);
      });

      expect(screen.getByTestId('loading')).toHaveTextContent('done');
      expect(screen.getByTestId('current-ws')).toHaveTextContent('Workspace 1');
    });

    it('goes back to loading when the session authenticates after mounting', async () => {
      authState = { ...authState, isAuthenticated: false };
      mockClient.getOrganizations.mockResolvedValueOnce(mockOrganizations);
      mockClient.getWorkspaces.mockResolvedValueOnce(mockWorkspaces);

      const { rerender } = render(
        <WorkspaceProvider useAuthHook={useAuthHook}>
          <TestComponent />
        </WorkspaceProvider>
      );

      await waitFor(() => {
        expect(screen.getByTestId('loading')).toHaveTextContent('done');
      });

      authState = { ...authState, isAuthenticated: true };
      rerender(
        <WorkspaceProvider useAuthHook={useAuthHook}>
          <TestComponent />
        </WorkspaceProvider>
      );

      expect(screen.getByTestId('loading')).toHaveTextContent('loading');
      await waitFor(() => {
        expect(screen.getByTestId('loading')).toHaveTextContent('done');
      });
      expect(screen.getByTestId('current-ws')).toHaveTextContent('Workspace 1');
    });

    it('restores organization from localStorage', async () => {
      localStorage.setItem('manager_current_org', 'org-2');
      mockClient.getOrganizations.mockResolvedValueOnce(mockOrganizations);
      mockClient.getWorkspaces.mockResolvedValueOnce([]);

      render(
        <WorkspaceProvider useAuthHook={useAuthHook}>
          <TestComponent />
        </WorkspaceProvider>
      );

      await waitFor(() => {
        expect(screen.getByTestId('current-org')).toHaveTextContent('Org 2');
      });
    });

    it('loads workspaces when organization is selected', async () => {
      mockClient.getOrganizations.mockResolvedValueOnce(mockOrganizations);
      mockClient.getWorkspaces.mockResolvedValueOnce(mockWorkspaces);

      render(
        <WorkspaceProvider useAuthHook={useAuthHook}>
          <TestComponent />
        </WorkspaceProvider>
      );

      await waitFor(() => {
        expect(screen.getByTestId('ws-count')).toHaveTextContent('2');
      });

      expect(screen.getByTestId('current-ws')).toHaveTextContent('Workspace 1');
    });

    it('restores workspace from localStorage', async () => {
      localStorage.setItem('manager_current_workspace', 'ws-2');
      mockClient.getOrganizations.mockResolvedValueOnce(mockOrganizations);
      mockClient.getWorkspaces.mockResolvedValueOnce(mockWorkspaces);

      render(
        <WorkspaceProvider useAuthHook={useAuthHook}>
          <TestComponent />
        </WorkspaceProvider>
      );

      await waitFor(() => {
        expect(screen.getByTestId('current-ws')).toHaveTextContent('Workspace 2');
      });
    });

    it('handles organization fetch error', async () => {
      mockClient.getOrganizations.mockRejectedValueOnce(new Error('Network error'));

      render(
        <WorkspaceProvider useAuthHook={useAuthHook}>
          <TestComponent />
        </WorkspaceProvider>
      );

      await waitFor(() => {
        expect(screen.getByTestId('error')).toHaveTextContent('Network error');
      });
    });

    it('handles workspace fetch error', async () => {
      mockClient.getOrganizations.mockResolvedValueOnce(mockOrganizations);
      mockClient.getWorkspaces.mockRejectedValueOnce(new Error('Workspace error'));

      render(
        <WorkspaceProvider useAuthHook={useAuthHook}>
          <TestComponent />
        </WorkspaceProvider>
      );

      await waitFor(() => {
        expect(screen.getByTestId('error')).toHaveTextContent('Workspace error');
      });
      expect(screen.getByTestId('loading')).toHaveTextContent('done');
    });

    it('handles empty organizations list', async () => {
      mockClient.getOrganizations.mockResolvedValueOnce([]);

      render(
        <WorkspaceProvider useAuthHook={useAuthHook}>
          <TestComponent />
        </WorkspaceProvider>
      );

      await waitFor(() => {
        expect(screen.getByTestId('loading')).toHaveTextContent('done');
      });

      expect(screen.getByTestId('orgs-count')).toHaveTextContent('0');
      expect(screen.getByTestId('current-org')).toHaveTextContent('none');
    });

    it('handles empty workspaces list', async () => {
      mockClient.getOrganizations.mockResolvedValueOnce(mockOrganizations);
      mockClient.getWorkspaces.mockResolvedValueOnce([]);

      render(
        <WorkspaceProvider useAuthHook={useAuthHook}>
          <TestComponent />
        </WorkspaceProvider>
      );

      await waitFor(() => {
        expect(screen.getByTestId('ws-count')).toHaveTextContent('0');
      });

      expect(screen.getByTestId('current-ws')).toHaveTextContent('none');
    });
  });

  describe('organization role', () => {
    const member = (user_id: string, role: 'owner' | 'admin' | 'member') => ({
      id: `member-${user_id}`,
      user_id,
      organization_id: 'org-1',
      role,
      email: '',
      display_name: null,
      joined_at: '',
    });

    it('keeps the role the organizations list already carries', async () => {
      mockClient.getOrganizations.mockResolvedValueOnce([
        { ...mockOrganizations[0], role: 'admin' },
      ]);
      mockClient.getWorkspaces.mockResolvedValueOnce([]);

      render(
        <WorkspaceProvider useAuthHook={useAuthHook}>
          <TestComponent />
        </WorkspaceProvider>
      );

      await waitFor(() => {
        expect(screen.getByTestId('current-role')).toHaveTextContent('admin');
      });
      expect(screen.getByTestId('resolving-role')).toHaveTextContent('settled');
      expect(mockClient.getOrgMembers).not.toHaveBeenCalled();
    });

    it('resolves a missing role from the membership list', async () => {
      mockClient.getOrganizations.mockResolvedValueOnce(mockOrganizations);
      mockClient.getWorkspaces.mockResolvedValue([]);
      let answer: (members: { members: ReturnType<typeof member>[] }) => void = () => {};
      mockClient.getOrgMembers.mockReturnValueOnce(
        new Promise((resolve) => {
          answer = resolve;
        })
      );

      render(
        <WorkspaceProvider useAuthHook={useAuthHook}>
          <TestComponent />
        </WorkspaceProvider>
      );

      await waitFor(() => {
        expect(screen.getByTestId('current-org')).toHaveTextContent('Org 1');
      });
      expect(screen.getByTestId('resolving-role')).toHaveTextContent('resolving');
      expect(screen.getByTestId('current-role')).toHaveTextContent('unknown');

      answer({ members: [member('user-2', 'admin'), member('user-1', 'owner')] });
      await waitFor(() => {
        expect(screen.getByTestId('current-role')).toHaveTextContent('owner');
      });
      expect(screen.getByTestId('resolving-role')).toHaveTextContent('settled');
      expect(mockClient.getOrgMembers).toHaveBeenCalledWith('org-1');
      expect(mockClient.getWorkspaces).toHaveBeenCalledTimes(1);
    });

    it('settles without a role when the membership list cannot be read', async () => {
      mockClient.getOrganizations.mockResolvedValueOnce(mockOrganizations);
      mockClient.getWorkspaces.mockResolvedValue([]);
      mockClient.getOrgMembers.mockRejectedValueOnce(new Error('offline'));

      render(
        <WorkspaceProvider useAuthHook={useAuthHook}>
          <TestComponent />
        </WorkspaceProvider>
      );

      await waitFor(() => {
        expect(screen.getByTestId('resolving-role')).toHaveTextContent('settled');
      });
      expect(screen.getByTestId('current-role')).toHaveTextContent('unknown');
      expect(mockClient.getOrgMembers).toHaveBeenCalledTimes(1);
    });
  });

  describe('setCurrentOrganization', () => {
    it('switches organization and clears workspace', async () => {
      mockClient.getOrganizations.mockResolvedValueOnce(mockOrganizations);
      mockClient.getWorkspaces.mockResolvedValueOnce(mockWorkspaces).mockResolvedValueOnce([]);

      render(
        <WorkspaceProvider useAuthHook={useAuthHook}>
          <TestComponent />
        </WorkspaceProvider>
      );

      await waitFor(() => {
        expect(screen.getByTestId('current-org')).toHaveTextContent('Org 1');
      });

      fireEvent.click(screen.getByText('Switch Org'));

      await waitFor(() => {
        expect(screen.getByTestId('current-org')).toHaveTextContent('Org 2');
      });
      expect(screen.getByTestId('current-ws')).toHaveTextContent('none');
      expect(localStorage.getItem('manager_current_org')).toBe('org-2');
      expect(localStorage.getItem('manager_current_workspace')).toBeNull();
    });
  });

  describe('setCurrentWorkspace', () => {
    it('switches workspace and persists to localStorage', async () => {
      mockClient.getOrganizations.mockResolvedValueOnce(mockOrganizations);
      mockClient.getWorkspaces.mockResolvedValueOnce(mockWorkspaces);

      render(
        <WorkspaceProvider useAuthHook={useAuthHook}>
          <TestComponent />
        </WorkspaceProvider>
      );

      await waitFor(() => {
        expect(screen.getByTestId('current-ws')).toHaveTextContent('Workspace 1');
      });

      fireEvent.click(screen.getByText('Switch Workspace'));

      expect(screen.getByTestId('current-ws')).toHaveTextContent('Workspace 2');
      expect(localStorage.getItem('manager_current_workspace')).toBe('ws-2');
    });
  });

  describe('refreshOrganizations', () => {
    it('refreshes organizations list', async () => {
      mockClient.getOrganizations.mockResolvedValueOnce(mockOrganizations).mockResolvedValueOnce([
        {
          id: 'org-3',
          name: 'Org 3',
          slug: 'org-3',
          description: null,
          is_active: true,
          created_at: '',
          updated_at: '',
        },
      ]);
      mockClient.getWorkspaces.mockResolvedValue([]);

      render(
        <WorkspaceProvider useAuthHook={useAuthHook}>
          <TestComponent />
        </WorkspaceProvider>
      );

      await waitFor(() => {
        expect(screen.getByTestId('orgs-count')).toHaveTextContent('2');
      });

      fireEvent.click(screen.getByText('Refresh Orgs'));

      await waitFor(() => {
        expect(screen.getByTestId('orgs-count')).toHaveTextContent('1');
      });

      expect(screen.getByTestId('current-org')).toHaveTextContent('Org 3');
    });
  });

  describe('refreshWorkspaces', () => {
    it('refreshes workspaces list', async () => {
      mockClient.getOrganizations.mockResolvedValueOnce(mockOrganizations);
      mockClient.getWorkspaces.mockResolvedValueOnce(mockWorkspaces).mockResolvedValueOnce([
        {
          id: 'ws-3',
          organization_id: 'org-1',
          name: 'Workspace 3',
          slug: 'ws-3',
          description: null,
          is_active: true,
          created_at: '',
          updated_at: '',
        },
      ]);

      render(
        <WorkspaceProvider useAuthHook={useAuthHook}>
          <TestComponent />
        </WorkspaceProvider>
      );

      await waitFor(() => {
        expect(screen.getByTestId('ws-count')).toHaveTextContent('2');
      });

      fireEvent.click(screen.getByText('Refresh Workspaces'));

      await waitFor(() => {
        expect(screen.getByTestId('ws-count')).toHaveTextContent('1');
      });
    });

    it('clears workspaces when no organization selected', async () => {
      mockClient.getOrganizations.mockResolvedValueOnce([]);

      render(
        <WorkspaceProvider useAuthHook={useAuthHook}>
          <TestComponent />
        </WorkspaceProvider>
      );

      await waitFor(() => {
        expect(screen.getByTestId('loading')).toHaveTextContent('done');
      });

      // refreshWorkspaces should clear workspaces when no org is selected
      expect(screen.getByTestId('ws-count')).toHaveTextContent('0');
    });

    it('calls refreshWorkspaces when no organization exists', async () => {
      // Start with no organizations
      mockClient.getOrganizations.mockResolvedValueOnce([]);

      render(
        <WorkspaceProvider useAuthHook={useAuthHook}>
          <TestComponent />
        </WorkspaceProvider>
      );

      await waitFor(() => {
        expect(screen.getByTestId('loading')).toHaveTextContent('done');
      });

      expect(screen.getByTestId('current-org')).toHaveTextContent('none');

      // Explicitly call refreshWorkspaces when no org is selected
      fireEvent.click(screen.getByText('Refresh Workspaces'));

      // Workspaces should remain empty
      expect(screen.getByTestId('ws-count')).toHaveTextContent('0');
      expect(screen.getByTestId('current-ws')).toHaveTextContent('none');
    });
  });

  describe('useWorkspace hook', () => {
    it('throws error when used outside provider', () => {
      const consoleError = console.error;
      console.error = mock();

      expect(() => {
        render(<TestComponent />);
      }).toThrow('useWorkspace must be used within a WorkspaceProvider');

      console.error = consoleError;
    });
  });
});
