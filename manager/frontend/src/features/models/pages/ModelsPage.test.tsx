import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { BrowserRouter } from 'react-router-dom';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { afterAll, beforeAll, beforeEach, describe, expect, it, mock } from 'bun:test';

// Create mock functions
const mockUseModels = mock();
const mockUseBrowse = mock();
const mockUsePull = mock();
const mockGetModelInfo = mock();

// Mock the hooks
mock.module('../hooks/useModels', () => ({
  useModels: mockUseModels,
}));

mock.module('../hooks/useBrowse', () => ({
  useBrowse: mockUseBrowse,
}));

mock.module('../hooks/usePull', () => ({
  usePull: mockUsePull,
}));

mock.module('../../../api/models', () => ({
  modelsApi: {
    getModelInfo: mockGetModelInfo,
  },
}));

// Mock VirtualBrowseList - pass the full model object to callbacks
mock.module('../components/VirtualBrowseList', () => ({
  default: function MockVirtualBrowseList({
    models,
    onItemClick,
    onInstall,
  }: {
    models: Array<{
      id: string;
      name: string;
      description?: string;
      downloads?: number;
      likes?: number;
      tags?: string[];
      author?: string;
      url?: string;
      install_name?: string;
    }>;
    onItemClick: (model: unknown) => void;
    onInstall: (model: unknown) => void;
  }) {
    return (
      <div data-testid="virtual-browse-list">
        {models.map((m) => (
          <div key={m.id} data-testid={`browse-model-${m.id}`}>
            <span>{m.name}</span>
            <button onClick={() => onItemClick(m)}>Details</button>
            <button onClick={() => onInstall(m)}>Install</button>
          </div>
        ))}
      </div>
    );
  },
}));

// Mock auth context
mock.module('../../../features/auth/context', () => ({
  useAuth: () => ({
    isAuthenticated: true,
    user: { id: '1', email: 'test@test.com' },
    roles: ['user'],
    permissions: ['models:read', 'models:delete'],
    hasPermission: () => true,
    hasAnyPermission: () => true,
    hasRole: () => true,
    logout: mock(),
    login: mock(),
  }),
  AuthProvider: ({ children }: { children: React.ReactNode }) => children,
}));

// Mock workspace context
mock.module('../../../shared/context/WorkspaceContext', () => ({
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

let ModelsPage: typeof import('./ModelsPage').default;

beforeAll(async () => {
  ModelsPage = (await import('./ModelsPage')).default;
});

afterAll(() => {
  mock.restore();
});

const defaultModelsHook = {
  models: [],
  loading: false,
  error: null,
  refresh: mock(),
  deleteModel: mock(),
};

const defaultBrowseHook = {
  source: 'all' as const,
  query: '',
  setQuery: mock(),
  models: [],
  loading: false,
  loadingMore: false,
  hasMore: true,
  error: null,
  search: mock(),
  loadMore: mock(),
  changeSource: mock(),
};

const defaultPullHook = {
  pulling: false,
  progress: null,
  steps: [],
  result: null,
  pull: mock(),
  reset: mock(),
  cancel: mock(),
};

const createWrapper = () => {
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: { retry: false },
      mutations: { retry: false },
    },
  });
  return ({ children }: { children: React.ReactNode }) => (
    <QueryClientProvider client={queryClient}>
      <BrowserRouter>{children}</BrowserRouter>
    </QueryClientProvider>
  );
};

const renderModelsPage = () => {
  const Wrapper = createWrapper();
  return render(
    <Wrapper>
      <ModelsPage />
    </Wrapper>
  );
};

describe('ModelsPage', () => {
  beforeEach(() => {
    mockUseModels.mockReset();
    mockUseBrowse.mockReset();
    mockUsePull.mockReset();
    mockGetModelInfo.mockReset();
    localStorage.clear();
    localStorage.setItem('accessToken', 'test-token');
    localStorage.setItem('user', JSON.stringify({ id: '1', email: 'test@test.com' }));
    localStorage.setItem('roles', JSON.stringify(['user']));
    localStorage.setItem('permissions', JSON.stringify(['models:read', 'models:delete']));
    mockUseModels.mockReturnValue(defaultModelsHook);
    mockUseBrowse.mockReturnValue(defaultBrowseHook);
    mockUsePull.mockReturnValue(defaultPullHook);
  });

  describe('rendering', () => {
    it('renders page header', () => {
      renderModelsPage();
      expect(screen.getByRole('heading', { name: 'Models' })).toBeInTheDocument();
    });

    it('renders main tabs', () => {
      renderModelsPage();
      expect(screen.getByRole('tab', { name: /Installed/i })).toBeInTheDocument();
      expect(screen.getByRole('tab', { name: 'Browse' })).toBeInTheDocument();
    });

    it('shows installed tab by default', () => {
      renderModelsPage();
      expect(screen.getByRole('heading', { name: 'Add Model' })).toBeInTheDocument();
      expect(screen.getByRole('heading', { name: 'Installed Models' })).toBeInTheDocument();
    });
  });

  describe('installed tab', () => {
    it('shows loading state', () => {
      mockUseModels.mockReturnValue({ ...defaultModelsHook, loading: true });
      renderModelsPage();
      expect(screen.getByText('Loading models...')).toBeInTheDocument();
    });

    it('shows error state', () => {
      mockUseModels.mockReturnValue({ ...defaultModelsHook, error: 'Failed to load' });
      renderModelsPage();
      expect(screen.getByText('Cannot connect to Ollama')).toBeInTheDocument();
    });

    it('shows empty state', () => {
      mockUseModels.mockReturnValue({ ...defaultModelsHook, models: [] });
      renderModelsPage();
      expect(screen.getByText('No models installed')).toBeInTheDocument();
    });

    it('displays installed models', () => {
      mockUseModels.mockReturnValue({
        ...defaultModelsHook,
        models: [
          { name: 'llama2', size: 3800000000, modified_at: '2024-01-01T00:00:00Z', digest: 'abc' },
          { name: 'mistral', size: 4000000000, modified_at: '2024-01-02T00:00:00Z', digest: 'def' },
        ],
      });
      renderModelsPage();
      expect(screen.getByText('llama2')).toBeInTheDocument();
      expect(screen.getByText('mistral')).toBeInTheDocument();
    });

    it('shows model badge count in tab', () => {
      mockUseModels.mockReturnValue({
        ...defaultModelsHook,
        models: [{ name: 'llama2', size: 1, modified_at: '', digest: '' }],
      });
      renderModelsPage();
      expect(screen.getByText('1')).toBeInTheDocument();
    });
  });

  describe('add model', () => {
    it('disables install button when input is empty', () => {
      renderModelsPage();
      const installButton = screen.getByRole('button', { name: 'Install' });
      expect(installButton).toBeDisabled();
    });

    it('enables install button when input has value', async () => {
      renderModelsPage();

      const input = screen.getByPlaceholderText('Model name...');
      fireEvent.change(input, { target: { value: 'llama2' } });

      const installButton = screen.getByRole('button', { name: 'Install' });
      expect(installButton).not.toBeDisabled();
    });

    it('calls pull when form submitted', async () => {
      const pullMock = mock(() => Promise.resolve(true));
      mockUsePull.mockReturnValue({ ...defaultPullHook, pull: pullMock });

      renderModelsPage();

      const input = screen.getByPlaceholderText('Model name...');
      fireEvent.change(input, { target: { value: 'llama2' } });
      fireEvent.click(screen.getByRole('button', { name: 'Install' }));

      await waitFor(() => {
        expect(pullMock).toHaveBeenCalledWith('llama2');
      });
    });

    it('shows pull progress', () => {
      mockUsePull.mockReturnValue({
        ...defaultPullHook,
        pulling: true,
        progress: 50,
      });
      renderModelsPage();

      expect(screen.getByText('Installing model...')).toBeInTheDocument();
      expect(screen.getByText('50%')).toBeInTheDocument();
    });

    it('shows pull steps', () => {
      mockUsePull.mockReturnValue({
        ...defaultPullHook,
        pulling: true,
        steps: [{ name: 'Downloading', message: 'In progress', status: 'pending' }],
      });
      renderModelsPage();

      expect(screen.getByText('Downloading')).toBeInTheDocument();
    });

    it('shows pull result success', () => {
      mockUsePull.mockReturnValue({
        ...defaultPullHook,
        result: { success: true, message: 'Model installed!' },
      });
      renderModelsPage();

      expect(screen.getByText('Installation complete')).toBeInTheDocument();
      expect(screen.getByText('Model installed!')).toBeInTheDocument();
    });

    it('shows pull result error', () => {
      mockUsePull.mockReturnValue({
        ...defaultPullHook,
        result: { success: false, message: 'Failed to install' },
      });
      renderModelsPage();

      expect(screen.getByText('Installation failed')).toBeInTheDocument();
    });
  });

  describe('delete model', () => {
    it('opens delete confirmation modal', async () => {
      mockUseModels.mockReturnValue({
        ...defaultModelsHook,
        models: [{ name: 'llama2', size: 1, modified_at: '', digest: '' }],
      });

      renderModelsPage();

      fireEvent.click(screen.getByTitle('Delete model'));

      await waitFor(() => {
        expect(screen.getByText('Delete Model')).toBeInTheDocument();
      });
      expect(screen.getByText(/Are you sure you want to delete/)).toBeInTheDocument();
    });

    it('calls deleteModel when confirmed', async () => {
      const deleteModelMock = mock(() => Promise.resolve(true));
      mockUseModels.mockReturnValue({
        ...defaultModelsHook,
        models: [{ name: 'llama2', size: 1, modified_at: '', digest: '' }],
        deleteModel: deleteModelMock,
      });

      renderModelsPage();

      fireEvent.click(screen.getByTitle('Delete model'));

      await waitFor(() => {
        expect(screen.getByText('Delete Model')).toBeInTheDocument();
      });

      fireEvent.click(screen.getByRole('button', { name: 'Delete' }));

      await waitFor(() => {
        expect(deleteModelMock).toHaveBeenCalledWith('llama2');
      });
    });

    it('closes modal when cancelled', async () => {
      mockUseModels.mockReturnValue({
        ...defaultModelsHook,
        models: [{ name: 'llama2', size: 1, modified_at: '', digest: '' }],
      });

      renderModelsPage();

      fireEvent.click(screen.getByTitle('Delete model'));

      await waitFor(() => {
        expect(screen.getByText('Delete Model')).toBeInTheDocument();
      });

      fireEvent.click(screen.getByRole('button', { name: 'Cancel' }));

      await waitFor(() => {
        expect(screen.queryByText('Delete Model')).not.toBeInTheDocument();
      });
    });
  });

  describe.skip('browse tab', () => {
    it('switches to browse tab', async () => {
      renderModelsPage();

      fireEvent.click(screen.getByRole('tab', { name: 'Browse' }));

      await waitFor(() => {
        expect(screen.getByTestId('virtual-browse-list')).toBeInTheDocument();
      });
    });

    it('triggers search on tab switch', async () => {
      const searchMock = mock();
      mockUseBrowse.mockReturnValue({ ...defaultBrowseHook, search: searchMock });

      renderModelsPage();

      fireEvent.click(screen.getByRole('tab', { name: 'Browse' }));

      await waitFor(() => {
        expect(searchMock).toHaveBeenCalled();
      });
    });

    it('shows source tabs', async () => {
      renderModelsPage();

      fireEvent.click(screen.getByRole('tab', { name: 'Browse' }));

      await waitFor(() => {
        expect(screen.getByText('All')).toBeInTheDocument();
      });
      expect(screen.getByText('Ollama')).toBeInTheDocument();
      expect(screen.getByText('HuggingFace')).toBeInTheDocument();
      expect(screen.getByText('GPT4All')).toBeInTheDocument();
      expect(screen.getByText('OpenRouter')).toBeInTheDocument();
    });

    it('changes source when tab clicked', async () => {
      const changeSourceMock = mock();
      mockUseBrowse.mockReturnValue({ ...defaultBrowseHook, changeSource: changeSourceMock });

      renderModelsPage();

      fireEvent.click(screen.getByRole('tab', { name: 'Browse' }));

      await waitFor(() => {
        expect(screen.getByText('HuggingFace')).toBeInTheDocument();
      });

      fireEvent.click(screen.getByText('HuggingFace'));

      await waitFor(() => {
        expect(changeSourceMock).toHaveBeenCalledWith('huggingface');
      });
    });

    it('shows loading state', async () => {
      mockUseBrowse.mockReturnValue({ ...defaultBrowseHook, loading: true });

      renderModelsPage();

      fireEvent.click(screen.getByRole('tab', { name: 'Browse' }));

      await waitFor(() => {
        expect(screen.getByText('Loading...')).toBeInTheDocument();
      });
    });

    it('shows error state', async () => {
      mockUseBrowse.mockReturnValue({ ...defaultBrowseHook, error: 'Search failed' });

      renderModelsPage();

      fireEvent.click(screen.getByRole('tab', { name: 'Browse' }));

      await waitFor(() => {
        expect(screen.getByText('Search failed')).toBeInTheDocument();
      });
    });

    it('searches when form submitted', async () => {
      const searchMock = mock();
      mockUseBrowse.mockReturnValue({ ...defaultBrowseHook, search: searchMock });

      renderModelsPage();

      fireEvent.click(screen.getByRole('tab', { name: 'Browse' }));

      await waitFor(() => {
        expect(screen.getByRole('button', { name: 'Search' })).toBeInTheDocument();
      });

      fireEvent.click(screen.getByRole('button', { name: 'Search' }));

      await waitFor(() => {
        expect(searchMock).toHaveBeenCalled();
      });
    });
  });

  describe('model details modal', () => {
    it('shows installed model details', async () => {
      mockUseModels.mockReturnValue({
        ...defaultModelsHook,
        models: [
          { name: 'llama2', size: 3800000000, modified_at: '2024-01-01T00:00:00Z', digest: 'abc' },
        ],
      });

      renderModelsPage();

      fireEvent.click(screen.getByText('llama2'));

      await waitFor(() => {
        expect(screen.getByText('Size')).toBeInTheDocument();
      });
      expect(screen.getByText('Modified')).toBeInTheDocument();
    });

    it('closes modal on close button click', async () => {
      mockUseModels.mockReturnValue({
        ...defaultModelsHook,
        models: [{ name: 'llama2', size: 1, modified_at: '', digest: '' }],
      });

      renderModelsPage();

      fireEvent.click(screen.getByText('llama2'));

      await waitFor(() => {
        expect(screen.getByText('Size')).toBeInTheDocument();
      });

      fireEvent.click(screen.getByLabelText('Close'));

      await waitFor(() => {
        expect(screen.queryByText('Modified')).not.toBeInTheDocument();
      });
    });
  });

  describe('refresh', () => {
    it('calls refresh when button clicked', async () => {
      const refreshMock = mock();
      mockUseModels.mockReturnValue({ ...defaultModelsHook, refresh: refreshMock });

      renderModelsPage();

      fireEvent.click(screen.getByTitle('Refresh'));

      await waitFor(() => {
        expect(refreshMock).toHaveBeenCalled();
      });
    });
  });

  describe.skip('browse model installation', () => {
    it('installs model from browse list', async () => {
      const pullMock = mock(() => Promise.resolve(true));
      mockUsePull.mockReturnValue({ ...defaultPullHook, pull: pullMock });
      mockUseBrowse.mockReturnValue({
        ...defaultBrowseHook,
        models: [
          {
            id: 'test-model',
            name: 'Test Model',
            description: 'A test',
            downloads: 1000,
            tags: [],
            install_name: 'test-install',
          },
        ],
      });

      renderModelsPage();

      fireEvent.click(screen.getByRole('tab', { name: 'Browse' }));

      await waitFor(() => {
        expect(screen.getByTestId('browse-model-test-model')).toBeInTheDocument();
      });

      fireEvent.click(screen.getByText('Install'));

      await waitFor(() => {
        expect(pullMock).toHaveBeenCalledWith('test-install');
      });
    });

    it('installs model using model name when no install_name', async () => {
      const pullMock = mock(() => Promise.resolve(true));
      mockUsePull.mockReturnValue({ ...defaultPullHook, pull: pullMock });
      mockUseBrowse.mockReturnValue({
        ...defaultBrowseHook,
        models: [
          {
            id: 'test-model',
            name: 'test-model-name',
            description: 'A test',
            downloads: 1000,
            tags: [],
          },
        ],
      });

      renderModelsPage();

      fireEvent.click(screen.getByRole('tab', { name: 'Browse' }));

      await waitFor(() => {
        expect(screen.getByTestId('browse-model-test-model')).toBeInTheDocument();
      });

      fireEvent.click(screen.getByText('Install'));

      await waitFor(() => {
        expect(pullMock).toHaveBeenCalledWith('test-model-name');
      });
    });
  });

  describe.skip('browse model details', () => {
    it('shows browse model details when clicked', async () => {
      mockUseBrowse.mockReturnValue({
        ...defaultBrowseHook,
        source: 'ollama',
        models: [
          {
            id: 'llama3',
            name: 'llama3:7b',
            size: 3800000000,
            details: {
              family: 'llama',
              parameter_size: '7B',
            },
          },
        ],
      });

      renderModelsPage();

      fireEvent.click(screen.getByRole('tab', { name: 'Browse' }));

      await waitFor(() => {
        expect(screen.getByTestId('browse-model-llama3')).toBeInTheDocument();
      });

      fireEvent.click(screen.getByText('Details'));

      await waitFor(() => {
        expect(screen.getByText('Size')).toBeInTheDocument();
        expect(screen.getByText('Family')).toBeInTheDocument();
        expect(screen.getByText('llama')).toBeInTheDocument();
      });
    });

    it('shows install button in browse model details', async () => {
      mockUseBrowse.mockReturnValue({
        ...defaultBrowseHook,
        source: 'ollama',
        models: [
          {
            id: 'llama3',
            name: 'llama3:7b',
            size: 3800000000,
          },
        ],
      });

      renderModelsPage();

      fireEvent.click(screen.getByRole('tab', { name: 'Browse' }));

      await waitFor(() => {
        expect(screen.getByTestId('browse-model-llama3')).toBeInTheDocument();
      });

      fireEvent.click(screen.getByText('Details'));

      await waitFor(() => {
        expect(screen.getByRole('button', { name: 'Install Model' })).toBeInTheDocument();
      });
    });

    it('shows model details tags when available', async () => {
      mockUseBrowse.mockReturnValue({
        ...defaultBrowseHook,
        source: 'ollama',
        models: [
          {
            id: 'model1',
            name: 'model1:latest',
            size: 1000000000,
            details: {
              family: 'mistral',
              parameter_size: '7B',
              quantization_level: 'Q4_0',
            },
          },
        ],
      });

      renderModelsPage();

      fireEvent.click(screen.getByRole('tab', { name: 'Browse' }));

      await waitFor(() => {
        expect(screen.getByTestId('browse-model-model1')).toBeInTheDocument();
      });

      fireEvent.click(screen.getByText('Details'));

      await waitFor(() => {
        expect(screen.getByText('mistral')).toBeInTheDocument();
        expect(screen.getByText('7B')).toBeInTheDocument();
        expect(screen.getByText('Q4_0')).toBeInTheDocument();
      });
    });

    it('fetches model info for HuggingFace models', async () => {
      mockGetModelInfo.mockResolvedValueOnce({
        content: '<p>Model card content</p>',
        gguf_size: 4000000000,
      });
      mockUseBrowse.mockReturnValue({
        ...defaultBrowseHook,
        source: 'huggingface',
        models: [
          {
            id: 'hf/model1',
            name: 'hf/model1',
            size: 4000000000,
            source: 'huggingface',
          },
        ],
      });

      renderModelsPage();

      fireEvent.click(screen.getByRole('tab', { name: 'Browse' }));

      await waitFor(() => {
        expect(screen.getByTestId('browse-model-hf/model1')).toBeInTheDocument();
      });

      fireEvent.click(screen.getByText('Details'));

      await waitFor(() => {
        expect(mockGetModelInfo).toHaveBeenCalledWith('hf/model1');
      });
    });

    it('GPT4All models do not fetch model card info', async () => {
      // GPT4All models don't fetch model card info (no README content)
      mockUseBrowse.mockReturnValue({
        ...defaultBrowseHook,
        source: 'gpt4all',
        models: [
          {
            id: 'gpt4all-model',
            name: 'gpt4all-model',
            size: 2000000000,
            source: 'gpt4all',
          },
        ],
      });

      renderModelsPage();

      fireEvent.click(screen.getByRole('tab', { name: 'Browse' }));

      await waitFor(() => {
        expect(screen.getByTestId('browse-model-gpt4all-model')).toBeInTheDocument();
      });

      fireEvent.click(screen.getByText('Details'));

      // GPT4All models should not trigger model info fetch
      await waitFor(() => {
        expect(mockGetModelInfo).not.toHaveBeenCalled();
      });
    });

    it('handles model info fetch error gracefully', async () => {
      mockGetModelInfo.mockRejectedValueOnce(new Error('Fetch failed'));
      mockUseBrowse.mockReturnValue({
        ...defaultBrowseHook,
        source: 'huggingface',
        models: [
          {
            id: 'hf/model1',
            name: 'hf/model1',
            size: 4000000000,
            source: 'huggingface',
          },
        ],
      });

      renderModelsPage();

      fireEvent.click(screen.getByRole('tab', { name: 'Browse' }));

      await waitFor(() => {
        expect(screen.getByTestId('browse-model-hf/model1')).toBeInTheDocument();
      });

      fireEvent.click(screen.getByText('Details'));

      // Should still show the model details without the model card
      await waitFor(() => {
        expect(screen.getByText('hf/model1')).toBeInTheDocument();
      });
    });
  });

  describe('delete model clears details', () => {
    it('closes details modal when deleted model was being viewed', async () => {
      const deleteModelMock = mock(() => Promise.resolve(true));
      mockUseModels.mockReturnValue({
        ...defaultModelsHook,
        models: [{ name: 'llama2', size: 1, modified_at: '2024-01-01', digest: 'abc' }],
        deleteModel: deleteModelMock,
      });

      renderModelsPage();

      // Open model details
      fireEvent.click(screen.getByText('llama2'));

      await waitFor(() => {
        expect(screen.getByText('Size')).toBeInTheDocument();
      });

      // Trigger delete from details modal
      fireEvent.click(screen.getByRole('button', { name: 'Delete Model' }));

      await waitFor(() => {
        expect(screen.getByText('Delete Model')).toBeInTheDocument();
      });

      // Confirm delete
      fireEvent.click(screen.getByRole('button', { name: 'Delete' }));

      await waitFor(() => {
        expect(deleteModelMock).toHaveBeenCalledWith('llama2');
      });
    });
  });

  describe('model item keyboard navigation', () => {
    it('shows model details on Enter key', async () => {
      mockUseModels.mockReturnValue({
        ...defaultModelsHook,
        models: [
          { name: 'llama2', size: 3800000000, modified_at: '2024-01-01T00:00:00Z', digest: 'abc' },
        ],
      });

      renderModelsPage();

      const modelItem = screen.getByText('llama2').closest('[role="button"]');
      fireEvent.keyDown(modelItem!, { key: 'Enter' });

      await waitFor(() => {
        expect(screen.getByText('Size')).toBeInTheDocument();
      });
    });
  });

  describe('modal keyboard handlers', () => {
    it('closes delete modal on Escape key', async () => {
      mockUseModels.mockReturnValue({
        ...defaultModelsHook,
        models: [{ name: 'llama2', size: 1, modified_at: '', digest: '' }],
      });

      renderModelsPage();

      fireEvent.click(screen.getByTitle('Delete model'));

      await waitFor(() => {
        expect(screen.getByText('Delete Model')).toBeInTheDocument();
      });

      // Press Escape to close modal
      fireEvent.keyDown(document, { key: 'Escape' });

      await waitFor(() => {
        expect(screen.queryByText('Delete Model')).not.toBeInTheDocument();
      });
    });

    it('closes details modal on Escape key', async () => {
      mockUseModels.mockReturnValue({
        ...defaultModelsHook,
        models: [{ name: 'llama2', size: 1, modified_at: '2024-01-01', digest: 'abc' }],
      });

      renderModelsPage();

      fireEvent.click(screen.getByText('llama2'));

      await waitFor(() => {
        expect(screen.getByText('Size')).toBeInTheDocument();
      });

      // Press Escape on the backdrop to close the details modal
      const backdrop = screen.getByLabelText('Close modal');
      fireEvent.keyDown(backdrop, { key: 'Escape' });

      await waitFor(() => {
        expect(screen.queryByText('Modified')).not.toBeInTheDocument();
      });
    });
  });

  describe.skip('source tab switching', () => {
    it('switches to GPT4All source', async () => {
      const changeSourceMock = mock();
      mockUseBrowse.mockReturnValue({ ...defaultBrowseHook, changeSource: changeSourceMock });

      renderModelsPage();

      fireEvent.click(screen.getByRole('tab', { name: 'Browse' }));

      await waitFor(() => {
        expect(screen.getByText('GPT4All')).toBeInTheDocument();
      });

      fireEvent.click(screen.getByText('GPT4All'));

      await waitFor(() => {
        expect(changeSourceMock).toHaveBeenCalledWith('gpt4all');
      });
    });

    it('switches to OpenRouter source', async () => {
      const changeSourceMock = mock();
      mockUseBrowse.mockReturnValue({ ...defaultBrowseHook, changeSource: changeSourceMock });

      renderModelsPage();

      fireEvent.click(screen.getByRole('tab', { name: 'Browse' }));

      await waitFor(() => {
        expect(screen.getByText('OpenRouter')).toBeInTheDocument();
      });

      fireEvent.click(screen.getByText('OpenRouter'));

      await waitFor(() => {
        expect(changeSourceMock).toHaveBeenCalledWith('openrouter');
      });
    });
  });

  describe('model family display', () => {
    it('shows model family when available', async () => {
      mockUseModels.mockReturnValue({
        ...defaultModelsHook,
        models: [
          {
            name: 'llama2',
            size: 3800000000,
            modified_at: '2024-01-01T00:00:00Z',
            digest: 'abc',
            details: { family: 'llama' },
          },
        ],
      });

      renderModelsPage();

      fireEvent.click(screen.getByText('llama2'));

      await waitFor(() => {
        expect(screen.getByText('Family')).toBeInTheDocument();
        expect(screen.getByText('llama')).toBeInTheDocument();
      });
    });
  });
});
