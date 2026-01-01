import { render, screen, fireEvent, waitFor } from '@testing-library/react';
import SourcesPage from './SourcesPage';
import { client } from '../api/client';
import type { Source } from '../types';

// Mock client
jest.mock('../api/client', () => ({
  client: {
    getSources: jest.fn(),
    getSource: jest.fn(),
    createSource: jest.fn(),
    updateSource: jest.fn(),
    deleteSource: jest.fn(),
    verifySource: jest.fn(),
  },
}));

const mockClient = client as jest.Mocked<typeof client>;

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
    jest.clearAllMocks();
    mockClient.getSources.mockResolvedValue(mockSources);
    // Mock window.confirm
    window.confirm = jest.fn(() => true);
  });

  it('shows loading state with skeleton cards', async () => {
    mockClient.getSources.mockImplementation(() => new Promise(() => {}));
    render(<SourcesPage />);
    expect(document.querySelectorAll('.skeleton-card').length).toBe(3);
  });

  it('shows empty state when no sources', async () => {
    mockClient.getSources.mockResolvedValueOnce([]);
    render(<SourcesPage />);
    await waitFor(() => {
      expect(screen.getByText('No sources configured. Add a source to get started!')).toBeInTheDocument();
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

  it('opens create source modal', async () => {
    render(<SourcesPage />);
    await waitFor(() => {
      expect(screen.getByRole('button', { name: '+ Add Source' })).toBeInTheDocument();
    });

    fireEvent.click(screen.getByRole('button', { name: '+ Add Source' }));
    expect(screen.getByRole('heading', { name: 'Add Source' })).toBeInTheDocument();
  });

  it('closes create modal on cancel', async () => {
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

  it('closes create modal on close button', async () => {
    render(<SourcesPage />);
    await waitFor(() => {
      expect(screen.getByRole('button', { name: '+ Add Source' })).toBeInTheDocument();
    });

    fireEvent.click(screen.getByRole('button', { name: '+ Add Source' }));
    fireEvent.click(screen.getByRole('button', { name: 'Close' }));
    await waitFor(() => {
      expect(screen.queryByRole('heading', { name: 'Add Source' })).not.toBeInTheDocument();
    });
  });

  it('verifies a source', async () => {
    mockClient.verifySource.mockResolvedValueOnce({ success: true, message: 'OK' });
    mockClient.getSource.mockResolvedValueOnce(mockSources[0]);

    render(<SourcesPage />);
    await waitFor(() => {
      expect(screen.getByText('GitHub Repository')).toBeInTheDocument();
    });

    const verifyButtons = screen.getAllByRole('button', { name: 'Verify' });
    fireEvent.click(verifyButtons[0]);

    await waitFor(() => {
      expect(mockClient.verifySource).toHaveBeenCalledWith('src-1');
    });
  });

  it('shows verification error', async () => {
    mockClient.verifySource.mockResolvedValueOnce({ success: false, message: 'Invalid credentials' });
    mockClient.getSource.mockResolvedValueOnce(mockSources[0]);

    render(<SourcesPage />);
    await waitFor(() => {
      expect(screen.getByText('GitHub Repository')).toBeInTheDocument();
    });

    const verifyButtons = screen.getAllByRole('button', { name: 'Verify' });
    fireEvent.click(verifyButtons[0]);

    await waitFor(() => {
      expect(screen.getByText('Verification failed: Invalid credentials')).toBeInTheDocument();
    });
  });

  it('disables an active source', async () => {
    const updatedSource = { ...mockSources[0], is_active: false };
    mockClient.updateSource.mockResolvedValueOnce(updatedSource);

    render(<SourcesPage />);
    await waitFor(() => {
      expect(screen.getByText('GitHub Repository')).toBeInTheDocument();
    });

    const disableButtons = screen.getAllByRole('button', { name: 'Disable' });
    fireEvent.click(disableButtons[0]);

    await waitFor(() => {
      expect(mockClient.updateSource).toHaveBeenCalledWith('src-1', { is_active: false });
    });
  });

  it('enables an inactive source', async () => {
    const updatedSource = { ...mockSources[1], is_active: true };
    mockClient.updateSource.mockResolvedValueOnce(updatedSource);

    render(<SourcesPage />);
    await waitFor(() => {
      expect(screen.getByText('GitLab Project')).toBeInTheDocument();
    });

    const enableButton = screen.getByRole('button', { name: 'Enable' });
    fireEvent.click(enableButton);

    await waitFor(() => {
      expect(mockClient.updateSource).toHaveBeenCalledWith('src-2', { is_active: true });
    });
  });

  it('deletes a source with confirmation', async () => {
    mockClient.deleteSource.mockResolvedValueOnce(undefined);

    render(<SourcesPage />);
    await waitFor(() => {
      expect(screen.getByText('GitHub Repository')).toBeInTheDocument();
    });

    const deleteButtons = screen.getAllByRole('button', { name: 'Delete' });
    fireEvent.click(deleteButtons[0]);

    expect(window.confirm).toHaveBeenCalled();
    await waitFor(() => {
      expect(mockClient.deleteSource).toHaveBeenCalledWith('src-1');
    });
  });

  it('cancels delete when confirm is rejected', async () => {
    (window.confirm as jest.Mock).mockReturnValueOnce(false);

    render(<SourcesPage />);
    await waitFor(() => {
      expect(screen.getByText('GitHub Repository')).toBeInTheDocument();
    });

    const deleteButtons = screen.getAllByRole('button', { name: 'Delete' });
    fireEvent.click(deleteButtons[0]);

    expect(window.confirm).toHaveBeenCalled();
    expect(mockClient.deleteSource).not.toHaveBeenCalled();
  });

  it('shows error when loading fails', async () => {
    mockClient.getSources.mockRejectedValueOnce(new Error('Network error'));
    render(<SourcesPage />);
    await waitFor(() => {
      expect(screen.getByText('Network error')).toBeInTheDocument();
    });
  });

  it('shows error when delete fails', async () => {
    mockClient.deleteSource.mockRejectedValueOnce(new Error('Delete failed'));

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
    mockClient.updateSource.mockRejectedValueOnce(new Error('Update failed'));

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
    mockClient.verifySource.mockRejectedValueOnce(new Error('Verify failed'));

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
    let resolveVerify: (value: { success: boolean; message: string }) => void;
    mockClient.verifySource.mockReturnValueOnce(
      new Promise((resolve) => {
        resolveVerify = resolve;
      })
    );
    mockClient.getSource.mockResolvedValueOnce(mockSources[0]);

    render(<SourcesPage />);
    await waitFor(() => {
      expect(screen.getByText('GitHub Repository')).toBeInTheDocument();
    });

    const verifyButtons = screen.getAllByRole('button', { name: 'Verify' });
    fireEvent.click(verifyButtons[0]);

    expect(screen.getByRole('button', { name: 'Verifying...' })).toBeDisabled();

    resolveVerify!({ success: true, message: 'OK' });

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

  it('shows source type cards in create modal', async () => {
    render(<SourcesPage />);
    await waitFor(() => {
      expect(screen.getByRole('button', { name: '+ Add Source' })).toBeInTheDocument();
    });

    fireEvent.click(screen.getByRole('button', { name: '+ Add Source' }));

    // Should show source type options
    expect(screen.getByText('Configuration')).toBeInTheDocument();
  });

  it('creates a new source', async () => {
    const newSource: Source = {
      id: 'src-3',
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
    mockClient.createSource.mockResolvedValueOnce(newSource);

    render(<SourcesPage />);
    await waitFor(() => {
      expect(screen.getByRole('button', { name: '+ Add Source' })).toBeInTheDocument();
    });

    fireEvent.click(screen.getByRole('button', { name: '+ Add Source' }));

    // Fill in form - GitHub is default
    const ownerInput = screen.getByLabelText(/Owner/);
    const repoInput = screen.getByLabelText(/Repository/);

    fireEvent.change(ownerInput, { target: { value: 'new' } });
    fireEvent.change(repoInput, { target: { value: 'repo' } });

    fireEvent.click(screen.getByRole('button', { name: 'Add Source' }));

    await waitFor(() => {
      expect(mockClient.createSource).toHaveBeenCalled();
    });
  });

  describe('FormFieldRenderer', () => {
    // Helper to find source type card by name
    const findSourceTypeCard = (name: string) => {
      const sourceTypeCards = document.querySelectorAll('.source-type-card');
      return Array.from(sourceTypeCards).find(card =>
        card.querySelector('.source-type-name')?.textContent === name
      );
    };

    it('renders toggle field for filesystem source', async () => {
      render(<SourcesPage />);
      await waitFor(() => {
        expect(screen.getByRole('button', { name: '+ Add Source' })).toBeInTheDocument();
      });

      fireEvent.click(screen.getByRole('button', { name: '+ Add Source' }));

      // Switch to filesystem source
      const filesystemCard = findSourceTypeCard('Filesystem');
      expect(filesystemCard).toBeDefined();
      fireEvent.click(filesystemCard!);

      // Check toggle field is rendered
      expect(screen.getByText('Allow write operations')).toBeInTheDocument();
      expect(screen.getByText('Enable agents to modify files in this directory')).toBeInTheDocument();

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
      const filesystemCard = findSourceTypeCard('Filesystem');
      fireEvent.click(filesystemCard!);

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
      const textCard = findSourceTypeCard('Text');
      expect(textCard).toBeDefined();
      fireEvent.click(textCard!);

      // Check textarea field is rendered
      expect(screen.getByLabelText(/Content/)).toBeInTheDocument();
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
      const textCard = findSourceTypeCard('Text');
      fireEvent.click(textCard!);

      const textarea = screen.getByPlaceholderText('Enter text content...');
      fireEvent.change(textarea, { target: { value: 'Some test content' } });
      expect(textarea).toHaveValue('Some test content');
    });

    it('changes source type and resets form', async () => {
      render(<SourcesPage />);
      await waitFor(() => {
        expect(screen.getByRole('button', { name: '+ Add Source' })).toBeInTheDocument();
      });

      fireEvent.click(screen.getByRole('button', { name: '+ Add Source' }));

      // Should start with GitHub
      expect(screen.getByLabelText(/Owner/)).toBeInTheDocument();

      // Switch to GitLab
      const gitlabCard = findSourceTypeCard('GitLab');
      expect(gitlabCard).toBeDefined();
      fireEvent.click(gitlabCard!);

      // Should now show GitLab fields
      expect(screen.getByLabelText(/^Project$/)).toBeInTheDocument();
      expect(screen.getByLabelText(/GitLab Host/)).toBeInTheDocument();
    });
  });

  describe('Create source error handling', () => {
    it('shows error when create source fails', async () => {
      mockClient.createSource.mockRejectedValueOnce(new Error('Creation failed'));

      render(<SourcesPage />);
      await waitFor(() => {
        expect(screen.getByRole('button', { name: '+ Add Source' })).toBeInTheDocument();
      });

      fireEvent.click(screen.getByRole('button', { name: '+ Add Source' }));

      const ownerInput = screen.getByLabelText(/Owner/);
      const repoInput = screen.getByLabelText(/Repository/);

      fireEvent.change(ownerInput, { target: { value: 'new' } });
      fireEvent.change(repoInput, { target: { value: 'repo' } });

      fireEvent.click(screen.getByRole('button', { name: 'Add Source' }));

      await waitFor(() => {
        expect(screen.getByText('Creation failed')).toBeInTheDocument();
      });
    });

    it('shows loading state during create source', async () => {
      mockClient.createSource.mockImplementation(() => new Promise(() => {}));

      render(<SourcesPage />);
      await waitFor(() => {
        expect(screen.getByRole('button', { name: '+ Add Source' })).toBeInTheDocument();
      });

      fireEvent.click(screen.getByRole('button', { name: '+ Add Source' }));

      const ownerInput = screen.getByLabelText(/Owner/);
      const repoInput = screen.getByLabelText(/Repository/);

      fireEvent.change(ownerInput, { target: { value: 'new' } });
      fireEvent.change(repoInput, { target: { value: 'repo' } });

      fireEvent.click(screen.getByRole('button', { name: 'Add Source' }));

      await waitFor(() => {
        expect(screen.getByRole('button', { name: 'Adding...' })).toBeDisabled();
      });
    });
  });
});
