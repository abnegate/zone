import { fireEvent, render, screen } from '@testing-library/react';
import { useWorkspace } from '../context/WorkspaceContext';
import ContextSwitcher from './ContextSwitcher';

// Mock useWorkspace
jest.mock('../context/WorkspaceContext', () => ({
  useWorkspace: jest.fn(),
}));

const mockUseWorkspace = useWorkspace as jest.Mock;

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

describe('ContextSwitcher', () => {
  const setCurrentOrganization = jest.fn();
  const setCurrentWorkspace = jest.fn();

  beforeEach(() => {
    jest.clearAllMocks();
    mockUseWorkspace.mockReturnValue({
      organizations: mockOrganizations,
      currentOrganization: mockOrganizations[0],
      workspaces: mockWorkspaces,
      currentWorkspace: mockWorkspaces[0],
      setCurrentOrganization,
      setCurrentWorkspace,
      loading: false,
    });
  });

  it('shows loading state', () => {
    mockUseWorkspace.mockReturnValue({
      loading: true,
      organizations: [],
      currentOrganization: null,
      workspaces: [],
      currentWorkspace: null,
      setCurrentOrganization,
      setCurrentWorkspace,
    });

    render(<ContextSwitcher />);
    expect(screen.getByText('Loading...')).toBeInTheDocument();
  });

  it('shows no organization state', () => {
    mockUseWorkspace.mockReturnValue({
      loading: false,
      organizations: [],
      currentOrganization: null,
      workspaces: [],
      currentWorkspace: null,
      setCurrentOrganization,
      setCurrentWorkspace,
    });

    render(<ContextSwitcher />);
    expect(screen.getByText('No organization')).toBeInTheDocument();
  });

  it('displays current organization name', () => {
    render(<ContextSwitcher />);
    expect(screen.getByText('Org 1')).toBeInTheDocument();
  });

  it('displays current workspace name when set', () => {
    render(<ContextSwitcher />);
    expect(screen.getByText('Workspace 1')).toBeInTheDocument();
  });

  it('opens dropdown on click', () => {
    render(<ContextSwitcher />);

    fireEvent.click(screen.getByRole('button', { expanded: false }));

    expect(screen.getByRole('listbox')).toBeInTheDocument();
    expect(screen.getByRole('heading', { name: 'Organizations' })).toBeInTheDocument();
    expect(screen.getByRole('heading', { name: 'Workspaces' })).toBeInTheDocument();
  });

  it('shows all organizations in dropdown', () => {
    render(<ContextSwitcher />);

    fireEvent.click(screen.getByRole('button', { expanded: false }));

    const options = screen.getAllByRole('option');
    expect(options).toHaveLength(4); // 2 orgs + 2 workspaces
  });

  it('calls setCurrentOrganization when org clicked', () => {
    render(<ContextSwitcher />);

    fireEvent.click(screen.getByRole('button', { expanded: false }));
    fireEvent.click(screen.getByText('Org 2'));

    expect(setCurrentOrganization).toHaveBeenCalledWith(mockOrganizations[1]);
  });

  it('calls setCurrentWorkspace when workspace clicked', () => {
    render(<ContextSwitcher />);

    fireEvent.click(screen.getByRole('button', { expanded: false }));
    fireEvent.click(screen.getByText('Workspace 2'));

    expect(setCurrentWorkspace).toHaveBeenCalledWith(mockWorkspaces[1]);
  });

  it('closes dropdown when item clicked', () => {
    render(<ContextSwitcher />);

    fireEvent.click(screen.getByRole('button', { expanded: false }));
    expect(screen.getByRole('listbox')).toBeInTheDocument();

    fireEvent.click(screen.getByText('Org 2'));
    expect(screen.queryByRole('listbox')).not.toBeInTheDocument();
  });

  it('hides workspaces section when no workspaces', () => {
    mockUseWorkspace.mockReturnValue({
      organizations: mockOrganizations,
      currentOrganization: mockOrganizations[0],
      workspaces: [],
      currentWorkspace: null,
      setCurrentOrganization,
      setCurrentWorkspace,
      loading: false,
    });

    render(<ContextSwitcher />);

    fireEvent.click(screen.getByRole('button', { expanded: false }));

    expect(screen.getByRole('heading', { name: 'Organizations' })).toBeInTheDocument();
    expect(screen.queryByRole('heading', { name: 'Workspaces' })).not.toBeInTheDocument();
  });

  it('closes dropdown on outside click', () => {
    render(
      <div>
        <ContextSwitcher />
        <button data-testid="outside">Outside</button>
      </div>
    );

    fireEvent.click(screen.getByRole('button', { expanded: false }));
    expect(screen.getByRole('listbox')).toBeInTheDocument();

    fireEvent.mouseDown(screen.getByTestId('outside'));
    expect(screen.queryByRole('listbox')).not.toBeInTheDocument();
  });
});
