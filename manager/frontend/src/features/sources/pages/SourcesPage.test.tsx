import { afterAll, beforeAll, beforeEach, describe, expect, it, mock } from 'bun:test';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import type { Source } from '../types';

// Create mock functions for the sources API
const mockGetSources = mock(() => Promise.resolve([] as Source[]));
const mockCreateSource = mock(() => Promise.resolve({} as Source));
const mockUpdateSource = mock(() => Promise.resolve({} as Source));
const mockDeleteSource = mock(() => Promise.resolve());
const mockVerifySource = mock(() => Promise.resolve({ verified: true, message: 'OK' }));
const mockGetSource = mock(() => Promise.resolve({} as Source));

// Mock sources API module
mock.module('../../../api/sources', () => ({
  sourcesApi: {
    getSources: mockGetSources,
    createSource: mockCreateSource,
    updateSource: mockUpdateSource,
    deleteSource: mockDeleteSource,
    verifySource: mockVerifySource,
    getSource: mockGetSource,
  },
}));

// Mock useWorkspace context - required by useSources hook
mock.module('../../../shared/context/WorkspaceContext', () => ({
  useWorkspace: () => ({
    currentWorkspace: { id: 'test-workspace-id', name: 'Test Workspace' },
    currentOrganization: { id: 'test-org-id', name: 'Test Org' },
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

let SourcesPage: typeof import('./SourcesPage').default;
let mockConfirm: ReturnType<typeof mock>;

beforeAll(async () => {
  SourcesPage = (await import('./SourcesPage')).default;
});

afterAll(() => {
  mock.restore();
});

const mockSources: Source[] = [
  {
    id: 'src-1',
    name: 'GitHub Repository',
    source_type: 'github',
    category: 'file',
    config: { owner: 'test', repo: 'repo' },
    url: 'https://github.com/test/repo',
    description: 'Main repository',
    is_active: true,
    last_verified_at: '2024-01-15T00:00:00Z',
    last_error: null,
    created_at: '2024-01-01T00:00:00Z',
    updated_at: '2024-01-15T00:00:00Z',
  },
  {
    id: 'src-2',
    name: 'GitLab Project',
    source_type: 'gitlab',
    category: 'file',
    config: { owner: 'test', repo: 'project' },
    url: 'https://gitlab.com/test/project',
    description: null,
    is_active: false,
    last_verified_at: null,
    last_error: null,
    created_at: '2024-01-02T00:00:00Z',
    updated_at: '2024-01-02T00:00:00Z',
  },
  {
    id: 'src-3',
    name: 'Error Source',
    source_type: 'web',
    category: 'web',
    config: { url: 'https://example.com' },
    url: 'https://example.com',
    description: null,
    is_active: true,
    last_verified_at: null,
    last_error: 'Authentication failed',
    created_at: '2024-01-03T00:00:00Z',
    updated_at: '2024-01-03T00:00:00Z',
  },
];

describe('SourcesPage', () => {
  beforeEach(() => {
    mockGetSources.mockReset();
    mockCreateSource.mockReset();
    mockUpdateSource.mockReset();
    mockDeleteSource.mockReset();
    mockVerifySource.mockReset();
    mockGetSource.mockReset();
    mockGetSources.mockImplementation(() => Promise.resolve(mockSources));
    // Mock window.confirm
    mockConfirm = mock(() => true);
    window.confirm = mockConfirm as typeof window.confirm;
  });

  it('shows loading state with skeleton cards', async () => {
    mockGetSources.mockImplementation(() => new Promise(() => {}));
    render(<SourcesPage />);
    expect(document.querySelectorAll('.skeleton-card').length).toBe(3);
  });

  it('shows empty state when no sources', async () => {
    mockGetSources.mockImplementation(() => Promise.resolve([]));
    render(<SourcesPage />);
    await waitFor(() => {
      expect(screen.getByText('No sources configured')).toBeInTheDocument();
    });
  });

  it('renders sources list', async () => {
    render(<SourcesPage />);
    await waitFor(() => {
      expect(screen.getByText('GitHub Repository')).toBeInTheDocument();
      expect(screen.getByText('GitLab Project')).toBeInTheDocument();
    });
  });

  it('renders page header', async () => {
    render(<SourcesPage />);
    await waitFor(() => {
      expect(screen.getByRole('heading', { name: 'Sources' })).toBeInTheDocument();
    });
    expect(screen.getByText(/Connect repositories, calendars/)).toBeInTheDocument();
  });

  it('renders Add Source button', async () => {
    render(<SourcesPage />);
    await waitFor(() => {
      expect(screen.getByRole('button', { name: '+ Add Source' })).toBeInTheDocument();
    });
  });

  it('displays source type badges', async () => {
    render(<SourcesPage />);
    await waitFor(() => {
      expect(screen.getByText('GitHub')).toBeInTheDocument();
      expect(screen.getByText('GitLab')).toBeInTheDocument();
    });
  });

  it('displays verified status', async () => {
    render(<SourcesPage />);
    await waitFor(() => {
      expect(screen.getByText('Verified')).toBeInTheDocument();
    });
  });

  it('displays error status', async () => {
    render(<SourcesPage />);
    await waitFor(() => {
      expect(screen.getByText('Error')).toBeInTheDocument();
    });
  });

  it('displays inactive status', async () => {
    render(<SourcesPage />);
    await waitFor(() => {
      expect(screen.getByText('Inactive')).toBeInTheDocument();
    });
  });

  it('displays source description', async () => {
    render(<SourcesPage />);
    await waitFor(() => {
      expect(screen.getByText('Main repository')).toBeInTheDocument();
    });
  });

  it('displays source URL as link', async () => {
    render(<SourcesPage />);
    await waitFor(() => {
      expect(screen.getByRole('link', { name: 'https://github.com/test/repo' })).toHaveAttribute(
        'href',
        'https://github.com/test/repo'
      );
    });
  });

  it('displays source error message', async () => {
    render(<SourcesPage />);
    await waitFor(() => {
      expect(screen.getByText('Authentication failed')).toBeInTheDocument();
    });
  });

  it('opens create source wizard', async () => {
    render(<SourcesPage />);
    await waitFor(() => {
      expect(screen.getByRole('button', { name: '+ Add Source' })).toBeInTheDocument();
    });

    fireEvent.click(screen.getByRole('button', { name: '+ Add Source' }));
    expect(screen.getByRole('heading', { name: 'Add Source' })).toBeInTheDocument();
  });

  it('closes create wizard on cancel', async () => {
    render(<SourcesPage />);
    await waitFor(() => {
      expect(screen.getByRole('button', { name: '+ Add Source' })).toBeInTheDocument();
    });

    fireEvent.click(screen.getByRole('button', { name: '+ Add Source' }));
    expect(screen.getByRole('heading', { name: 'Add Source' })).toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: 'Cancel' }));
    await waitFor(() => {
      expect(screen.queryByRole('heading', { name: 'Add Source' })).not.toBeInTheDocument();
    });
  });

  it('closes create wizard on close button', async () => {
    render(<SourcesPage />);
    await waitFor(() => {
      expect(screen.getByRole('button', { name: '+ Add Source' })).toBeInTheDocument();
    });

    fireEvent.click(screen.getByRole('button', { name: '+ Add Source' }));
    fireEvent.click(screen.getByRole('button', { name: 'Close wizard' }));
    await waitFor(() => {
      expect(screen.queryByRole('heading', { name: 'Add Source' })).not.toBeInTheDocument();
    });
  });

  it('verifies a source', async () => {
    mockVerifySource.mockImplementation(() => Promise.resolve({ verified: true, message: 'OK' }));
    mockGetSource.mockImplementation(() => Promise.resolve(mockSources[0]));

    render(<SourcesPage />);
    await waitFor(() => {
      expect(screen.getByText('GitHub Repository')).toBeInTheDocument();
    });

    const verifyButtons = screen.getAllByRole('button', { name: 'Verify' });
    fireEvent.click(verifyButtons[0]);

    await waitFor(() => {
      expect(mockVerifySource).toHaveBeenCalledWith('test-workspace-id', 'src-1');
      expect(mockGetSource).toHaveBeenCalledWith('test-workspace-id', 'src-1');
      expect(screen.queryByRole('button', { name: 'Verifying...' })).not.toBeInTheDocument();
      expect(screen.queryByRole('alert')).not.toBeInTheDocument();
    });
  });

  it('shows the verification rejection message and refreshed error status', async () => {
    mockVerifySource.mockImplementation(() =>
      Promise.resolve({ verified: false, message: 'Repository access denied' })
    );
    mockGetSource.mockImplementation(() =>
      Promise.resolve({
        ...mockSources[0],
        last_error: 'Repository access denied',
      })
    );

    render(<SourcesPage />);
    await screen.findByText('GitHub Repository');
    fireEvent.click(screen.getAllByRole('button', { name: 'Verify' })[0]);

    await waitFor(() => {
      expect(screen.getByRole('alert')).toHaveTextContent('Repository access denied');
      expect(screen.getAllByText('Error')).toHaveLength(2);
      expect(screen.queryByText('Verified')).not.toBeInTheDocument();
    });
  });

  it('disables an active source', async () => {
    const updatedSource = { ...mockSources[0], is_active: false };
    mockUpdateSource.mockImplementation(() => Promise.resolve(updatedSource));

    render(<SourcesPage />);
    await waitFor(() => {
      expect(screen.getByText('GitHub Repository')).toBeInTheDocument();
    });

    const disableButtons = screen.getAllByRole('button', { name: 'Disable' });
    fireEvent.click(disableButtons[0]);

    await waitFor(() => {
      expect(mockUpdateSource).toHaveBeenCalledWith('test-workspace-id', 'src-1', {
        is_active: false,
      });
    });
  });

  it('enables an inactive source', async () => {
    const updatedSource = { ...mockSources[1], is_active: true };
    mockUpdateSource.mockImplementation(() => Promise.resolve(updatedSource));

    render(<SourcesPage />);
    await waitFor(() => {
      expect(screen.getByText('GitLab Project')).toBeInTheDocument();
    });

    const enableButton = screen.getByRole('button', { name: 'Enable' });
    fireEvent.click(enableButton);

    await waitFor(() => {
      expect(mockUpdateSource).toHaveBeenCalledWith('test-workspace-id', 'src-2', {
        is_active: true,
      });
    });
  });

  it('deletes a source with confirmation', async () => {
    mockDeleteSource.mockImplementation(() => Promise.resolve(undefined));

    render(<SourcesPage />);
    await waitFor(() => {
      expect(screen.getByText('GitHub Repository')).toBeInTheDocument();
    });

    const deleteButtons = screen.getAllByRole('button', { name: 'Delete' });
    fireEvent.click(deleteButtons[0]);

    expect(window.confirm).toHaveBeenCalled();
    await waitFor(() => {
      expect(mockDeleteSource).toHaveBeenCalledWith('test-workspace-id', 'src-1');
    });
  });

  it('cancels delete when confirm is rejected', async () => {
    mockConfirm.mockReturnValueOnce(false);

    render(<SourcesPage />);
    await waitFor(() => {
      expect(screen.getByText('GitHub Repository')).toBeInTheDocument();
    });

    const deleteButtons = screen.getAllByRole('button', { name: 'Delete' });
    fireEvent.click(deleteButtons[0]);

    expect(window.confirm).toHaveBeenCalled();
    expect(mockDeleteSource).not.toHaveBeenCalled();
  });

  it('shows error when loading fails', async () => {
    mockGetSources.mockImplementation(() => Promise.reject(new Error('Network error')));
    render(<SourcesPage />);
    await waitFor(() => {
      expect(screen.getByText('Network error')).toBeInTheDocument();
    });
  });

  it('shows error when delete fails', async () => {
    mockDeleteSource.mockImplementation(() => Promise.reject(new Error('Delete failed')));

    render(<SourcesPage />);
    await waitFor(() => {
      expect(screen.getByText('GitHub Repository')).toBeInTheDocument();
    });

    const deleteButtons = screen.getAllByRole('button', { name: 'Delete' });
    fireEvent.click(deleteButtons[0]);

    await waitFor(() => {
      expect(screen.getByText('Delete failed')).toBeInTheDocument();
    });
  });

  it('shows error when toggle active fails', async () => {
    mockUpdateSource.mockImplementation(() => Promise.reject(new Error('Update failed')));

    render(<SourcesPage />);
    await waitFor(() => {
      expect(screen.getByText('GitHub Repository')).toBeInTheDocument();
    });

    const disableButtons = screen.getAllByRole('button', { name: 'Disable' });
    fireEvent.click(disableButtons[0]);

    await waitFor(() => {
      expect(screen.getByText('Update failed')).toBeInTheDocument();
    });
  });

  it('shows error when verify fails', async () => {
    mockVerifySource.mockImplementation(() => Promise.reject(new Error('Verify failed')));

    render(<SourcesPage />);
    await waitFor(() => {
      expect(screen.getByText('GitHub Repository')).toBeInTheDocument();
    });

    const verifyButtons = screen.getAllByRole('button', { name: 'Verify' });
    fireEvent.click(verifyButtons[0]);

    await waitFor(() => {
      expect(screen.getByText('Verify failed')).toBeInTheDocument();
    });
  });

  it('shows verifying state during verification', async () => {
    let resolveVerify!: (value: { verified: boolean; message: string }) => void;
    mockVerifySource.mockImplementation(
      () =>
        new Promise((resolve) => {
          resolveVerify = resolve;
        })
    );
    mockGetSource.mockImplementation(() => Promise.resolve(mockSources[0]));

    render(<SourcesPage />);
    await waitFor(() => {
      expect(screen.getByText('GitHub Repository')).toBeInTheDocument();
    });

    const verifyButtons = screen.getAllByRole('button', { name: 'Verify' });
    fireEvent.click(verifyButtons[0]);

    expect(screen.getByRole('button', { name: 'Verifying...' })).toBeDisabled();

    resolveVerify({ verified: true, message: 'OK' });

    await waitFor(() => {
      expect(screen.getAllByRole('button', { name: 'Verify' }).length).toBeGreaterThan(0);
    });
  });

  it('displays verified date', async () => {
    render(<SourcesPage />);
    await waitFor(() => {
      expect(screen.getByText(/Verified:/)).toBeInTheDocument();
    });
  });

  it('applies inactive styling to inactive sources', async () => {
    render(<SourcesPage />);
    await waitFor(() => {
      expect(screen.getByText('GitLab Project')).toBeInTheDocument();
    });

    const inactiveCard = screen.getByText('GitLab Project').closest('.source-card');
    expect(inactiveCard).toHaveClass('source-inactive');
  });

  it('shows source type options in create wizard', async () => {
    render(<SourcesPage />);
    await waitFor(() => {
      expect(screen.getByRole('button', { name: '+ Add Source' })).toBeInTheDocument();
    });

    fireEvent.click(screen.getByRole('button', { name: '+ Add Source' }));

    // Should show source type selection (first step of wizard)
    expect(screen.getByText('Source Type')).toBeInTheDocument();
  });

  it('creates a new source via wizard', async () => {
    const newSource: Source = {
      id: 'src-4',
      name: 'New Repo',
      source_type: 'github',
      category: 'file',
      config: { owner: 'new', repo: 'repo' },
      url: 'https://github.com/new/repo',
      description: null,
      is_active: true,
      last_verified_at: null,
      last_error: null,
      created_at: '2024-01-17T00:00:00Z',
      updated_at: '2024-01-17T00:00:00Z',
    };
    mockCreateSource.mockImplementation(() => Promise.resolve(newSource));

    render(<SourcesPage />);
    await waitFor(() => {
      expect(screen.getByRole('button', { name: '+ Add Source' })).toBeInTheDocument();
    });

    fireEvent.click(screen.getByRole('button', { name: '+ Add Source' }));

    // Step 1: Source type is already GitHub (default), click Next
    fireEvent.click(screen.getByRole('button', { name: 'Next' }));

    // Step 2: Fill in configuration
    await waitFor(() => {
      expect(screen.getByLabelText(/Owner/)).toBeInTheDocument();
    });
    const ownerInput = screen.getByLabelText(/Owner/);
    const repoInput = screen.getByLabelText(/Repository/);

    fireEvent.change(ownerInput, { target: { value: 'new' } });
    fireEvent.change(repoInput, { target: { value: 'repo' } });

    // Click Next to go to details step
    fireEvent.click(screen.getByRole('button', { name: 'Next' }));

    // Step 3: Details - click Add Source to complete
    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'Add Source' })).toBeInTheDocument();
    });
    fireEvent.click(screen.getByRole('button', { name: 'Add Source' }));

    await waitFor(() => {
      expect(mockCreateSource).toHaveBeenCalledWith(
        'test-workspace-id',
        expect.objectContaining({
          name: 'new/repo',
          source_type: 'github',
          config: { owner: 'new', repo: 'repo', branch: 'main' },
          url: 'https://github.com/new/repo',
        })
      );
    });
  });

  describe('FormFieldRenderer in wizard', () => {
    // Helper to find source type option by name in wizard
    const findSourceTypeOption = (name: string) => {
      const sourceTypeOptions = document.querySelectorAll('.source-type-option');
      return Array.from(sourceTypeOptions).find(
        (option) => option.querySelector('.source-type-name')?.textContent === name
      );
    };

    it('renders toggle field for filesystem source', async () => {
      render(<SourcesPage />);
      await waitFor(() => {
        expect(screen.getByRole('button', { name: '+ Add Source' })).toBeInTheDocument();
      });

      fireEvent.click(screen.getByRole('button', { name: '+ Add Source' }));

      // Switch to filesystem source in step 1
      const filesystemOption = findSourceTypeOption('Filesystem');
      expect(filesystemOption).toBeDefined();
      fireEvent.click(filesystemOption!);

      // Go to step 2 (configuration)
      fireEvent.click(screen.getByRole('button', { name: 'Next' }));

      // Check toggle field is rendered
      await waitFor(() => {
        expect(screen.getByText('Allow write operations')).toBeInTheDocument();
      });
      expect(
        screen.getByText('Enable agents to modify files in this directory')
      ).toBeInTheDocument();

      // Toggle is rendered as checkbox
      const toggleCheckbox = screen.getByRole('checkbox');
      expect(toggleCheckbox).toBeChecked(); // default is true
    });

    it('allows toggling the toggle field', async () => {
      render(<SourcesPage />);
      await waitFor(() => {
        expect(screen.getByRole('button', { name: '+ Add Source' })).toBeInTheDocument();
      });

      fireEvent.click(screen.getByRole('button', { name: '+ Add Source' }));

      // Switch to filesystem source
      const filesystemOption = findSourceTypeOption('Filesystem');
      fireEvent.click(filesystemOption!);

      // Go to step 2 (configuration)
      fireEvent.click(screen.getByRole('button', { name: 'Next' }));

      await waitFor(() => {
        expect(screen.getByRole('checkbox')).toBeInTheDocument();
      });

      const toggleCheckbox = screen.getByRole('checkbox');
      expect(toggleCheckbox).toBeChecked();

      // Toggle off
      fireEvent.click(toggleCheckbox);
      expect(toggleCheckbox).not.toBeChecked();

      // Toggle back on
      fireEvent.click(toggleCheckbox);
      expect(toggleCheckbox).toBeChecked();
    });

    it('renders textarea field for text source', async () => {
      render(<SourcesPage />);
      await waitFor(() => {
        expect(screen.getByRole('button', { name: '+ Add Source' })).toBeInTheDocument();
      });

      fireEvent.click(screen.getByRole('button', { name: '+ Add Source' }));

      // Switch to text source
      const textOption = findSourceTypeOption('Text');
      expect(textOption).toBeDefined();
      fireEvent.click(textOption!);

      // Go to step 2 (configuration)
      fireEvent.click(screen.getByRole('button', { name: 'Next' }));

      // Check textarea field is rendered
      await waitFor(() => {
        expect(screen.getByLabelText(/Content/)).toBeInTheDocument();
      });
      const textarea = screen.getByPlaceholderText('Enter text content...');
      expect(textarea.tagName).toBe('TEXTAREA');
    });

    it('allows input in textarea field', async () => {
      render(<SourcesPage />);
      await waitFor(() => {
        expect(screen.getByRole('button', { name: '+ Add Source' })).toBeInTheDocument();
      });

      fireEvent.click(screen.getByRole('button', { name: '+ Add Source' }));

      // Switch to text source
      const textOption = findSourceTypeOption('Text');
      fireEvent.click(textOption!);

      // Go to step 2 (configuration)
      fireEvent.click(screen.getByRole('button', { name: 'Next' }));

      await waitFor(() => {
        expect(screen.getByPlaceholderText('Enter text content...')).toBeInTheDocument();
      });

      const textarea = screen.getByPlaceholderText('Enter text content...');
      fireEvent.change(textarea, { target: { value: 'Some test content' } });
      expect(textarea).toHaveValue('Some test content');
    });

    it('changes source type and shows different config fields', async () => {
      render(<SourcesPage />);
      await waitFor(() => {
        expect(screen.getByRole('button', { name: '+ Add Source' })).toBeInTheDocument();
      });

      fireEvent.click(screen.getByRole('button', { name: '+ Add Source' }));

      // GitHub is selected by default, go to config step
      fireEvent.click(screen.getByRole('button', { name: 'Next' }));

      // Should show GitHub fields
      await waitFor(() => {
        expect(screen.getByLabelText(/Owner/)).toBeInTheDocument();
      });

      // Go back to step 1
      fireEvent.click(screen.getByRole('button', { name: 'Previous' }));

      // Switch to GitLab
      await waitFor(() => {
        const gitlabOption = findSourceTypeOption('GitLab');
        expect(gitlabOption).toBeDefined();
        fireEvent.click(gitlabOption!);
      });

      // Go to config step again
      fireEvent.click(screen.getByRole('button', { name: 'Next' }));

      // Should now show GitLab fields
      await waitFor(() => {
        expect(screen.getByLabelText(/^Project$/)).toBeInTheDocument();
        expect(screen.getByLabelText(/GitLab Host/)).toBeInTheDocument();
      });
    });
  });

  describe('Create source error handling', () => {
    it('shows error when create source fails', async () => {
      mockCreateSource.mockImplementation(() => Promise.reject(new Error('Creation failed')));

      render(<SourcesPage />);
      await waitFor(() => {
        expect(screen.getByRole('button', { name: '+ Add Source' })).toBeInTheDocument();
      });

      fireEvent.click(screen.getByRole('button', { name: '+ Add Source' }));

      // Step 1: Next to config
      fireEvent.click(screen.getByRole('button', { name: 'Next' }));

      await waitFor(() => {
        expect(screen.getByLabelText(/Owner/)).toBeInTheDocument();
      });

      const ownerInput = screen.getByLabelText(/Owner/);
      const repoInput = screen.getByLabelText(/Repository/);

      fireEvent.change(ownerInput, { target: { value: 'new' } });
      fireEvent.change(repoInput, { target: { value: 'repo' } });

      // Step 2: Next to details
      fireEvent.click(screen.getByRole('button', { name: 'Next' }));

      // Step 3: Submit
      await waitFor(() => {
        expect(screen.getByRole('button', { name: 'Add Source' })).toBeInTheDocument();
      });
      fireEvent.click(screen.getByRole('button', { name: 'Add Source' }));

      await waitFor(() => {
        expect(screen.getByText('Creation failed')).toBeInTheDocument();
      });
    });

    it('shows loading state during create source', async () => {
      mockCreateSource.mockImplementation(() => new Promise(() => {}));

      render(<SourcesPage />);
      await waitFor(() => {
        expect(screen.getByRole('button', { name: '+ Add Source' })).toBeInTheDocument();
      });

      fireEvent.click(screen.getByRole('button', { name: '+ Add Source' }));

      // Step 1: Next to config
      fireEvent.click(screen.getByRole('button', { name: 'Next' }));

      await waitFor(() => {
        expect(screen.getByLabelText(/Owner/)).toBeInTheDocument();
      });

      const ownerInput = screen.getByLabelText(/Owner/);
      const repoInput = screen.getByLabelText(/Repository/);

      fireEvent.change(ownerInput, { target: { value: 'new' } });
      fireEvent.change(repoInput, { target: { value: 'repo' } });

      // Step 2: Next to details
      fireEvent.click(screen.getByRole('button', { name: 'Next' }));

      // Step 3: Submit
      await waitFor(() => {
        expect(screen.getByRole('button', { name: 'Add Source' })).toBeInTheDocument();
      });
      fireEvent.click(screen.getByRole('button', { name: 'Add Source' }));

      await waitFor(() => {
        expect(screen.getByRole('button', { name: 'Adding...' })).toBeDisabled();
      });
    });
  });
});
