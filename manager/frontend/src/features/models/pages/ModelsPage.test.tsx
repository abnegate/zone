import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { BrowserRouter } from 'react-router-dom';
import { modelsApi } from '../../../api/models';
import { AuthProvider } from '../../../features/auth';
import ModelsPage from './ModelsPage';

// Mock the hooks
jest.mock('../hooks/useModels', () => ({
  useModels: jest.fn(),
}));

jest.mock('../hooks/useBrowse', () => ({
  useBrowse: jest.fn(),
}));

jest.mock('../hooks/usePull', () => ({
  usePull: jest.fn(),
}));

jest.mock('../../../api/models', () => ({
  modelsApi: {
    getModelInfo: jest.fn(),
  },
}));

// Mock VirtualBrowseList - pass the full model object to callbacks
jest.mock('../components/VirtualBrowseList', () => {
  return function MockVirtualBrowseList({
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
  };
});

import { useBrowse } from '../hooks/useBrowse';
import { useModels } from '../hooks/useModels';
import { usePull } from '../hooks/usePull';

const mockUseModels = useModels as jest.Mock;
const mockUseBrowse = useBrowse as jest.Mock;
const mockUsePull = usePull as jest.Mock;
const mockModelsApi = modelsApi as jest.Mocked<typeof modelsApi>;

const setupAuth = () => {
  localStorage.setItem('accessToken', 'test-token');
  localStorage.setItem('user', JSON.stringify({ id: '1', email: 'test@test.com' }));
  localStorage.setItem('roles', JSON.stringify(['user']));
  localStorage.setItem('permissions', JSON.stringify(['models:read', 'models:delete']));
};

const defaultModelsHook = {
  models: [],
  loading: false,
  error: null,
  refresh: jest.fn(),
  deleteModel: jest.fn(),
};

const defaultBrowseHook = {
  source: 'ollama' as const,
  query: '',
  setQuery: jest.fn(),
  models: [],
  loading: false,
  loadingMore: false,
  hasMore: true,
  error: null,
  search: jest.fn(),
  loadMore: jest.fn(),
  changeSource: jest.fn(),
};

const defaultPullHook = {
  pulling: false,
  progress: null,
  steps: [],
  result: null,
  pull: jest.fn(),
  reset: jest.fn(),
  cancel: jest.fn(),
};

const renderModelsPage = () => {
  return render(
    <BrowserRouter>
      <AuthProvider>
        <ModelsPage />
      </AuthProvider>
    </BrowserRouter>
  );
};

describe('ModelsPage', () => {
  beforeEach(() => {
    jest.clearAllMocks();
    localStorage.clear();
    setupAuth();
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
      expect(screen.getByRole('button', { name: /Installed/i })).toBeInTheDocument();
      expect(screen.getByRole('button', { name: 'Browse' })).toBeInTheDocument();
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
      expect(screen.getByText('Failed to load')).toBeInTheDocument();
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
      const pullMock = jest.fn().mockResolvedValue(true);
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
      const deleteModelMock = jest.fn().mockResolvedValue(true);
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

  describe('browse tab', () => {
    it('switches to browse tab', async () => {
      renderModelsPage();

      fireEvent.click(screen.getByRole('button', { name: 'Browse' }));

      await waitFor(() => {
        expect(screen.getByTestId('virtual-browse-list')).toBeInTheDocument();
      });
    });

    it('triggers search on tab switch', async () => {
      const searchMock = jest.fn();
      mockUseBrowse.mockReturnValue({ ...defaultBrowseHook, search: searchMock });

      renderModelsPage();

      fireEvent.click(screen.getByRole('button', { name: 'Browse' }));

      await waitFor(() => {
        expect(searchMock).toHaveBeenCalled();
      });
    });

    it('shows source tabs', async () => {
      renderModelsPage();

      fireEvent.click(screen.getByRole('button', { name: 'Browse' }));

      await waitFor(() => {
        expect(screen.getByText('Ollama Library')).toBeInTheDocument();
      });
      expect(screen.getByText('HuggingFace')).toBeInTheDocument();
      expect(screen.getByText('ModelScope')).toBeInTheDocument();
    });

    it('changes source when tab clicked', async () => {
      const changeSourceMock = jest.fn();
      mockUseBrowse.mockReturnValue({ ...defaultBrowseHook, changeSource: changeSourceMock });

      renderModelsPage();

      fireEvent.click(screen.getByRole('button', { name: 'Browse' }));

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

      fireEvent.click(screen.getByRole('button', { name: 'Browse' }));

      await waitFor(() => {
        expect(screen.getByText('Loading...')).toBeInTheDocument();
      });
    });

    it('shows error state', async () => {
      mockUseBrowse.mockReturnValue({ ...defaultBrowseHook, error: 'Search failed' });

      renderModelsPage();

      fireEvent.click(screen.getByRole('button', { name: 'Browse' }));

      await waitFor(() => {
        expect(screen.getByText('Search failed')).toBeInTheDocument();
      });
    });

    it('searches when form submitted', async () => {
      const searchMock = jest.fn();
      mockUseBrowse.mockReturnValue({ ...defaultBrowseHook, search: searchMock });

      renderModelsPage();

      fireEvent.click(screen.getByRole('button', { name: 'Browse' }));

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
      const refreshMock = jest.fn();
      mockUseModels.mockReturnValue({ ...defaultModelsHook, refresh: refreshMock });

      renderModelsPage();

      fireEvent.click(screen.getByTitle('Refresh'));

      await waitFor(() => {
        expect(refreshMock).toHaveBeenCalled();
      });
    });
  });

  describe('browse model installation', () => {
    it('installs model from browse list', async () => {
      const pullMock = jest.fn().mockResolvedValue(true);
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

      fireEvent.click(screen.getByRole('button', { name: 'Browse' }));

      await waitFor(() => {
        expect(screen.getByTestId('browse-model-test-model')).toBeInTheDocument();
      });

      fireEvent.click(screen.getByText('Install'));

      await waitFor(() => {
        expect(pullMock).toHaveBeenCalledWith('test-install');
      });
    });

    it('installs model using model name when no install_name', async () => {
      const pullMock = jest.fn().mockResolvedValue(true);
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

      fireEvent.click(screen.getByRole('button', { name: 'Browse' }));

      await waitFor(() => {
        expect(screen.getByTestId('browse-model-test-model')).toBeInTheDocument();
      });

      fireEvent.click(screen.getByText('Install'));

      await waitFor(() => {
        expect(pullMock).toHaveBeenCalledWith('test-model-name');
      });
    });
  });

  describe('browse model details', () => {
    it('shows browse model details when clicked', async () => {
      mockUseBrowse.mockReturnValue({
        ...defaultBrowseHook,
        source: 'ollama',
        models: [
          {
            id: 'llama3',
            name: 'Llama 3',
            description: 'A powerful model',
            downloads: 1500000,
            likes: 5000,
            tags: ['llm', 'meta'],
            author: 'Meta',
            url: 'https://ollama.com/llama3',
          },
        ],
      });

      renderModelsPage();

      fireEvent.click(screen.getByRole('button', { name: 'Browse' }));

      await waitFor(() => {
        expect(screen.getByTestId('browse-model-llama3')).toBeInTheDocument();
      });

      fireEvent.click(screen.getByText('Details'));

      await waitFor(() => {
        expect(screen.getByText('A powerful model')).toBeInTheDocument();
        expect(screen.getByText('Meta')).toBeInTheDocument();
        expect(screen.getByText('Downloads')).toBeInTheDocument();
      });
    });

    it('shows install button in browse model details', async () => {
      mockUseBrowse.mockReturnValue({
        ...defaultBrowseHook,
        source: 'ollama',
        models: [
          {
            id: 'llama3',
            name: 'Llama 3',
            description: 'A powerful model',
            downloads: 1000,
            tags: [],
          },
        ],
      });

      renderModelsPage();

      fireEvent.click(screen.getByRole('button', { name: 'Browse' }));

      await waitFor(() => {
        expect(screen.getByTestId('browse-model-llama3')).toBeInTheDocument();
      });

      fireEvent.click(screen.getByText('Details'));

      await waitFor(() => {
        expect(screen.getByRole('button', { name: 'Install Model' })).toBeInTheDocument();
      });
    });

    it('shows model tags in details', async () => {
      mockUseBrowse.mockReturnValue({
        ...defaultBrowseHook,
        models: [
          {
            id: 'model1',
            name: 'Model 1',
            description: 'Test',
            downloads: 100,
            tags: ['llm', 'chat'],
          },
        ],
      });

      renderModelsPage();

      fireEvent.click(screen.getByRole('button', { name: 'Browse' }));

      await waitFor(() => {
        expect(screen.getByTestId('browse-model-model1')).toBeInTheDocument();
      });

      fireEvent.click(screen.getByText('Details'));

      await waitFor(() => {
        expect(screen.getByText('llm')).toBeInTheDocument();
        expect(screen.getByText('chat')).toBeInTheDocument();
      });
    });

    it('fetches model info for HuggingFace models', async () => {
      mockModelsApi.getModelInfo.mockResolvedValueOnce({
        content: '<p>Model card content</p>',
        gguf_size: 4000000000,
      });
      mockUseBrowse.mockReturnValue({
        ...defaultBrowseHook,
        source: 'huggingface',
        models: [
          {
            id: 'hf/model1',
            name: 'HF Model',
            description: 'HuggingFace model',
            downloads: 500,
            tags: [],
          },
        ],
      });

      renderModelsPage();

      fireEvent.click(screen.getByRole('button', { name: 'Browse' }));

      await waitFor(() => {
        expect(screen.getByTestId('browse-model-hf/model1')).toBeInTheDocument();
      });

      fireEvent.click(screen.getByText('Details'));

      await waitFor(() => {
        expect(mockModelsApi.getModelInfo).toHaveBeenCalledWith('hf/model1');
      });
    });

    it('fetches model info for ModelScope models', async () => {
      mockModelsApi.getModelInfo.mockResolvedValueOnce({
        content: '<p>Model card</p>',
        gguf_size: 2000000000,
      });
      mockUseBrowse.mockReturnValue({
        ...defaultBrowseHook,
        source: 'modelscope',
        models: [
          {
            id: 'ms-model',
            name: 'ModelScope Model',
            description: 'A model',
            downloads: 100,
            tags: [],
          },
        ],
      });

      renderModelsPage();

      fireEvent.click(screen.getByRole('button', { name: 'Browse' }));

      await waitFor(() => {
        expect(screen.getByTestId('browse-model-ms-model')).toBeInTheDocument();
      });

      fireEvent.click(screen.getByText('Details'));

      await waitFor(() => {
        expect(mockModelsApi.getModelInfo).toHaveBeenCalledWith('modelscope/ms-model');
      });
    });

    it('handles model info fetch error gracefully', async () => {
      mockModelsApi.getModelInfo.mockRejectedValueOnce(new Error('Fetch failed'));
      mockUseBrowse.mockReturnValue({
        ...defaultBrowseHook,
        source: 'huggingface',
        models: [
          {
            id: 'hf/model1',
            name: 'HF Model',
            description: 'Test model',
            downloads: 500,
            tags: [],
          },
        ],
      });

      renderModelsPage();

      fireEvent.click(screen.getByRole('button', { name: 'Browse' }));

      await waitFor(() => {
        expect(screen.getByTestId('browse-model-hf/model1')).toBeInTheDocument();
      });

      fireEvent.click(screen.getByText('Details'));

      // Should still show the model details without the model card
      await waitFor(() => {
        expect(screen.getByText('Test model')).toBeInTheDocument();
      });
    });
  });

  describe('delete model clears details', () => {
    it('closes details modal when deleted model was being viewed', async () => {
      const deleteModelMock = jest.fn().mockResolvedValue(true);
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

  describe('source tab switching', () => {
    it('switches to ModelScope source', async () => {
      const changeSourceMock = jest.fn();
      mockUseBrowse.mockReturnValue({ ...defaultBrowseHook, changeSource: changeSourceMock });

      renderModelsPage();

      fireEvent.click(screen.getByRole('button', { name: 'Browse' }));

      await waitFor(() => {
        expect(screen.getByText('ModelScope')).toBeInTheDocument();
      });

      fireEvent.click(screen.getByText('ModelScope'));

      await waitFor(() => {
        expect(changeSourceMock).toHaveBeenCalledWith('modelscope');
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
