import { beforeEach, describe, expect, it } from 'bun:test';
import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import ContextSwitcher from './ContextSwitcher';

const mockOrganizations = [
  { id: 'org-1', name: 'Org 1', slug: 'org-1', is_active: true, created_at: '', updated_at: '' },
  { id: 'org-2', name: 'Org 2', slug: 'org-2', is_active: true, created_at: '', updated_at: '' },
];

const mockWorkspaces = [
  {
    id: 'ws-1',
    organization_id: 'org-1',
    name: 'Workspace 1',
    slug: 'ws-1',
    is_active: true,
    created_at: '',
    updated_at: '',
  },
  {
    id: 'ws-2',
    organization_id: 'org-1',
    name: 'Workspace 2',
    slug: 'ws-2',
    is_active: true,
    created_at: '',
    updated_at: '',
  },
];

type WorkspaceState = {
  organizations: typeof mockOrganizations;
  currentOrganization: (typeof mockOrganizations)[number] | null;
  workspaces: typeof mockWorkspaces;
  currentWorkspace: (typeof mockWorkspaces)[number] | null;
  setCurrentOrganization: (org: (typeof mockOrganizations)[number]) => void;
  setCurrentWorkspace: (ws: (typeof mockWorkspaces)[number]) => void;
  refreshOrganizations: () => Promise<void>;
  loading: boolean;
  error: string | null;
};

let setCurrentOrganizationCalls: Array<(typeof mockOrganizations)[number]> = [];
let setCurrentWorkspaceCalls: Array<(typeof mockWorkspaces)[number]> = [];
let refreshOrganizationsCalls = 0;

const refreshOrganizations = async () => {
  refreshOrganizationsCalls += 1;
};

const setCurrentOrganization = (org: (typeof mockOrganizations)[number]) => {
  setCurrentOrganizationCalls.push(org);
};

const setCurrentWorkspace = (ws: (typeof mockWorkspaces)[number]) => {
  setCurrentWorkspaceCalls.push(ws);
};

let workspaceState: WorkspaceState;

const useWorkspaceHook = () => workspaceState;

describe('ContextSwitcher', () => {
  beforeEach(() => {
    setCurrentOrganizationCalls = [];
    setCurrentWorkspaceCalls = [];
    refreshOrganizationsCalls = 0;
    workspaceState = {
      organizations: mockOrganizations,
      currentOrganization: mockOrganizations[0],
      workspaces: mockWorkspaces,
      currentWorkspace: mockWorkspaces[0],
      setCurrentOrganization,
      setCurrentWorkspace,
      refreshOrganizations,
      loading: false,
      error: null,
    };
  });

  it('shows a neutral placeholder while the organizations are still loading', () => {
    workspaceState = {
      ...workspaceState,
      organizations: [],
      currentOrganization: null,
      workspaces: [],
      currentWorkspace: null,
      loading: true,
    };

    render(<ContextSwitcher useWorkspaceHook={useWorkspaceHook} />);
    const placeholder = screen.getByRole('status', { name: 'Loading organizations' });
    expect(placeholder).toHaveClass('context-switcher-placeholder');
    expect(placeholder).toHaveTextContent('');
    expect(screen.queryByText('Loading...')).toBeNull();
    expect(screen.queryByText('No organization')).toBeNull();
    expect(screen.queryByRole('button')).toBeNull();
  });

  it('keeps the button once the organization is known while its workspaces load', () => {
    workspaceState = { ...workspaceState, workspaces: [], currentWorkspace: null, loading: true };

    render(<ContextSwitcher useWorkspaceHook={useWorkspaceHook} />);
    expect(screen.getByRole('button', { expanded: false })).toHaveTextContent('Org 1');
    expect(screen.queryByRole('status')).toBeNull();
  });

  it('offers to select an organization once none resolved', async () => {
    const user = userEvent.setup();
    workspaceState = {
      ...workspaceState,
      organizations: [],
      currentOrganization: null,
      workspaces: [],
      currentWorkspace: null,
    };

    render(<ContextSwitcher useWorkspaceHook={useWorkspaceHook} />);
    const button = screen.getByRole('button', { name: 'Select organization' });
    expect(button).toHaveClass('context-switcher-button');
    expect(screen.queryByText('No organization')).toBeNull();

    await user.click(button);
    expect(screen.getByRole('heading', { name: 'Organizations' })).toBeInTheDocument();
    expect(screen.getByText('No organizations yet')).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Retry' })).toBeNull();
  });

  it('retries the organizations request from the dropdown after it failed', async () => {
    const user = userEvent.setup();
    workspaceState = {
      ...workspaceState,
      organizations: [],
      currentOrganization: null,
      workspaces: [],
      currentWorkspace: null,
      error: 'Network error',
    };

    render(<ContextSwitcher useWorkspaceHook={useWorkspaceHook} />);
    await user.click(screen.getByRole('button', { name: 'Select organization' }));
    expect(screen.getByText('Could not load')).toBeInTheDocument();

    await user.click(screen.getByRole('button', { name: 'Retry' }));
    expect(refreshOrganizationsCalls).toBe(1);
  });

  it('displays current organization name', () => {
    render(<ContextSwitcher useWorkspaceHook={useWorkspaceHook} />);
    expect(screen.getByText('Org 1')).toBeInTheDocument();
  });

  it('displays current workspace name when set', () => {
    render(<ContextSwitcher useWorkspaceHook={useWorkspaceHook} />);
    expect(screen.getByText('Workspace 1')).toBeInTheDocument();
  });

  it('shows the organization as an eyebrow above the workspace name', () => {
    render(<ContextSwitcher useWorkspaceHook={useWorkspaceHook} />);
    const label = screen.getByRole('button', { expanded: false }).querySelector('.context-label');
    expect(label?.children[0]).toHaveClass('org-name');
    expect(label?.children[0]).toHaveTextContent('Org 1');
    expect(label?.children[1]).toHaveClass('ws-name');
    expect(label?.children[1]).toHaveTextContent('Workspace 1');
    expect(label?.querySelector('.separator')).toBeNull();
  });

  it('opens dropdown on click', async () => {
    const user = userEvent.setup();
    render(<ContextSwitcher useWorkspaceHook={useWorkspaceHook} />);

    await user.click(screen.getByRole('button', { expanded: false }));

    expect(screen.getByRole('listbox')).toBeInTheDocument();
    expect(screen.getByRole('heading', { name: 'Organizations' })).toBeInTheDocument();
    expect(screen.getByRole('heading', { name: 'Workspaces' })).toBeInTheDocument();
  });

  it('shows all organizations in dropdown', async () => {
    const user = userEvent.setup();
    render(<ContextSwitcher useWorkspaceHook={useWorkspaceHook} />);

    await user.click(screen.getByRole('button', { expanded: false }));

    const options = screen.getAllByRole('option');
    expect(options).toHaveLength(4); // 2 orgs + 2 workspaces
  });

  it('calls setCurrentOrganization when org clicked', async () => {
    const user = userEvent.setup();
    render(<ContextSwitcher useWorkspaceHook={useWorkspaceHook} />);

    await user.click(screen.getByRole('button', { expanded: false }));
    await user.click(screen.getByText('Org 2'));

    await waitFor(() => {
      expect(setCurrentOrganizationCalls).toEqual([mockOrganizations[1]]);
    });
  });

  it('calls setCurrentWorkspace when workspace clicked', async () => {
    const user = userEvent.setup();
    render(<ContextSwitcher useWorkspaceHook={useWorkspaceHook} />);

    await user.click(screen.getByRole('button', { expanded: false }));
    await user.click(screen.getByText('Workspace 2'));

    await waitFor(() => {
      expect(setCurrentWorkspaceCalls).toEqual([mockWorkspaces[1]]);
    });
  });

  it('closes dropdown when item clicked', async () => {
    const user = userEvent.setup();
    render(<ContextSwitcher useWorkspaceHook={useWorkspaceHook} />);

    await user.click(screen.getByRole('button', { expanded: false }));
    expect(screen.getByRole('listbox')).toBeInTheDocument();

    await user.click(screen.getByText('Org 2'));
    await waitFor(() => {
      expect(screen.queryByRole('listbox')).not.toBeInTheDocument();
    });
  });

  it('hides workspaces section when no workspaces', async () => {
    const user = userEvent.setup();
    workspaceState = { ...workspaceState, workspaces: [], currentWorkspace: null };

    render(<ContextSwitcher useWorkspaceHook={useWorkspaceHook} />);

    await user.click(screen.getByRole('button', { expanded: false }));

    expect(screen.getByRole('heading', { name: 'Organizations' })).toBeInTheDocument();
    expect(screen.queryByRole('heading', { name: 'Workspaces' })).not.toBeInTheDocument();
  });

  it('closes dropdown on outside click', async () => {
    const user = userEvent.setup();
    render(
      <div>
        <ContextSwitcher useWorkspaceHook={useWorkspaceHook} />
        <button data-testid="outside">Outside</button>
      </div>
    );

    await user.click(screen.getByRole('button', { expanded: false }));
    expect(screen.getByRole('listbox')).toBeInTheDocument();

    await user.click(screen.getByTestId('outside'));
    await waitFor(() => {
      expect(screen.queryByRole('listbox')).not.toBeInTheDocument();
    });
  });
});
