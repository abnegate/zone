import { fireEvent, render, screen, waitFor, cleanup } from '@testing-library/react';
import { BrowserRouter } from 'react-router-dom';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { afterEach, afterAll, mock, beforeAll, beforeEach, describe, it, expect } from 'bun:test';
import type { KnowledgeEntry } from '../types';

// Create mock functions for useKnowledge hook
const mockCreateEntry = mock();
const mockDeleteEntry = mock();
const mockRefreshEntry = mock();
const mockReload = mock();

// Create a function reference that we can update
let getMockState: () => { entries: KnowledgeEntry[]; loading: boolean; error: string | null };

// Mock the useKnowledge hook directly - use a callback to get current state
mock.module('../hooks', () => ({
  useKnowledge: () => {
    const state = getMockState();
    return {
      entries: state.entries,
      loading: state.loading,
      error: state.error,
      refreshing: null,
      createEntry: mockCreateEntry,
      deleteEntry: mockDeleteEntry,
      refreshEntry: mockRefreshEntry,
      reload: mockReload,
    };
  },
  // Re-export useContextSearch for module compatibility
  useContextSearch: () => ({
    results: [],
    total: 0,
    loading: false,
    error: null,
    search: mock(),
    clear: mock(),
  }),
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

let WikiPage: typeof import('./WikiPage').default;

beforeAll(async () => {
  WikiPage = (await import('./WikiPage')).default;
});

afterAll(() => {
  mock.restore();
});

afterEach(() => {
  cleanup();
});

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

const renderWikiPage = () => {
  const Wrapper = createWrapper();
  return render(
    <Wrapper>
      <WikiPage />
    </Wrapper>
  );
};

describe('WikiPage', () => {
  const defaultEntries: KnowledgeEntry[] = [
    {
      id: 'kb-1',
      workspace_id: 'ws-1',
      title: 'Text Entry',
      type: 'text',
      content: 'This is text content',
      fetched_content: null,
      tags: ['tag1', 'tag2'],
      last_refreshed_at: null,
      created_at: '2024-01-01T00:00:00Z',
      updated_at: '2024-01-01T00:00:00Z',
    },
    {
      id: 'kb-2',
      workspace_id: 'ws-1',
      title: 'URL Entry',
      type: 'url',
      content: 'https://example.com',
      fetched_content: 'Fetched content from URL',
      tags: ['documentation'],
      last_refreshed_at: '2024-01-02T00:00:00Z',
      created_at: '2024-01-01T00:00:00Z',
      updated_at: '2024-01-02T00:00:00Z',
    },
  ];

  beforeEach(() => {
    mockCreateEntry.mockReset();
    mockDeleteEntry.mockReset();
    mockRefreshEntry.mockReset();
    mockReload.mockReset();
    // Set default state for each test
    getMockState = () => ({
      entries: defaultEntries,
      loading: false,
      error: null,
    });
  });

  describe('Initial Load', () => {
    it('renders page heading and subtitle', () => {
      renderWikiPage();
      expect(screen.getByRole('heading', { name: 'Knowledge Base' })).toBeInTheDocument();
      expect(
        screen.getByText('Manage documentation, links, and content for your AI models')
      ).toBeInTheDocument();
    });

    it('displays knowledge entries', () => {
      renderWikiPage();
      expect(screen.getByText('Text Entry')).toBeInTheDocument();
      expect(screen.getByText('URL Entry')).toBeInTheDocument();
    });

    it('shows loading state initially', () => {
      getMockState = () => ({ entries: [], loading: true, error: null });
      renderWikiPage();
      expect(screen.getByText('Loading knowledge...')).toBeInTheDocument();
    });

    it('displays error message on load failure', () => {
      getMockState = () => ({ entries: [], loading: false, error: 'Failed to load' });
      renderWikiPage();
      expect(screen.getByText('Failed to load')).toBeInTheDocument();
    });

    it('displays empty state when no entries', () => {
      getMockState = () => ({ entries: [], loading: false, error: null });
      renderWikiPage();
      expect(screen.getByText('No knowledge entries found')).toBeInTheDocument();
      expect(
        screen.getByText('Add your first knowledge entry to build your knowledge base')
      ).toBeInTheDocument();
    });
  });

  describe('Search Functionality', () => {
    it('renders search input', () => {
      renderWikiPage();
      expect(screen.getByPlaceholderText('Search knowledge...')).toBeInTheDocument();
    });

    it('filters entries by title', () => {
      renderWikiPage();
      const searchInput = screen.getByPlaceholderText('Search knowledge...');
      fireEvent.change(searchInput, { target: { value: 'Text' } });
      expect(screen.getByText('Text Entry')).toBeInTheDocument();
      expect(screen.queryByText('URL Entry')).not.toBeInTheDocument();
    });

    it('filters entries by content', () => {
      renderWikiPage();
      const searchInput = screen.getByPlaceholderText('Search knowledge...');
      fireEvent.change(searchInput, { target: { value: 'Fetched content' } });
      expect(screen.queryByText('Text Entry')).not.toBeInTheDocument();
      expect(screen.getByText('URL Entry')).toBeInTheDocument();
    });

    it('filters entries by tags', () => {
      renderWikiPage();
      const searchInput = screen.getByPlaceholderText('Search knowledge...');
      fireEvent.change(searchInput, { target: { value: 'documentation' } });
      expect(screen.queryByText('Text Entry')).not.toBeInTheDocument();
      expect(screen.getByText('URL Entry')).toBeInTheDocument();
    });

    it('shows appropriate message when search yields no results', () => {
      renderWikiPage();
      const searchInput = screen.getByPlaceholderText('Search knowledge...');
      fireEvent.change(searchInput, { target: { value: 'nonexistent' } });
      expect(screen.getByText('No knowledge entries found')).toBeInTheDocument();
      expect(screen.getByText('Try adjusting your filters or search query')).toBeInTheDocument();
    });
  });

  describe('Filter Functionality', () => {
    it('renders filter tabs', () => {
      renderWikiPage();
      expect(screen.getByRole('tab', { name: 'All' })).toBeInTheDocument();
      expect(screen.getByRole('tab', { name: 'Text' })).toBeInTheDocument();
      expect(screen.getByRole('tab', { name: 'URL' })).toBeInTheDocument();
    });

    it.skip('filters by text type', () => {
      renderWikiPage();
      const textFilter = screen.getByRole('tab', { name: 'Text' });
      fireEvent.click(textFilter);
      expect(screen.getByText('Text Entry')).toBeInTheDocument();
      expect(screen.queryByText('URL Entry')).not.toBeInTheDocument();
    });

    it.skip('filters by url type', () => {
      renderWikiPage();
      const urlFilter = screen.getByRole('tab', { name: 'URL' });
      fireEvent.click(urlFilter);
      expect(screen.queryByText('Text Entry')).not.toBeInTheDocument();
      expect(screen.getByText('URL Entry')).toBeInTheDocument();
    });

    it.skip('shows all entries when All filter is active', () => {
      renderWikiPage();
      const urlFilter = screen.getByRole('tab', { name: 'URL' });
      fireEvent.click(urlFilter);
      const allFilter = screen.getByRole('tab', { name: 'All' });
      fireEvent.click(allFilter);
      expect(screen.getByText('Text Entry')).toBeInTheDocument();
      expect(screen.getByText('URL Entry')).toBeInTheDocument();
    });

    it.skip('applies data-state to selected filter', () => {
      renderWikiPage();
      const textFilter = screen.getByRole('tab', { name: 'Text' });
      expect(textFilter).not.toHaveAttribute('data-state', 'active');
      fireEvent.click(textFilter);
      expect(textFilter).toHaveAttribute('data-state', 'active');
    });
  });

  describe('Knowledge Entry Display', () => {
    it('displays entry cards with correct information', () => {
      renderWikiPage();
      expect(screen.getByText('Text Entry')).toBeInTheDocument();
      expect(screen.getByText('This is text content')).toBeInTheDocument();
      expect(screen.getByText('tag1')).toBeInTheDocument();
      expect(screen.getByText('tag2')).toBeInTheDocument();
    });

    it('displays type badge for text entries', () => {
      renderWikiPage();
      // Badge component displays the type
      const badges = screen.getAllByText('text');
      expect(badges.length).toBeGreaterThan(0);
    });

    it('displays type badge for URL entries', () => {
      renderWikiPage();
      // Badge component displays the type
      const badges = screen.getAllByText('url');
      expect(badges.length).toBeGreaterThan(0);
    });

    it('displays URL link for URL entries', () => {
      renderWikiPage();
      const link = screen.getByText('https://example.com') as HTMLAnchorElement;
      expect(link.href).toBe('https://example.com/');
    });

    it('displays fetched content for URL entries', () => {
      renderWikiPage();
      expect(screen.getByText('Fetched content from URL')).toBeInTheDocument();
    });

    it('shows refresh button only for URL entries', () => {
      renderWikiPage();
      const refreshButtons = screen.getAllByLabelText('Refresh URL content');
      expect(refreshButtons.length).toBe(1);
    });
  });

  describe.skip('Create Knowledge Wizard', () => {
    it('opens create wizard when Add Knowledge button is clicked', () => {
      renderWikiPage();
      const addButton = screen.getAllByText('Add Knowledge')[0];
      fireEvent.click(addButton);
      expect(screen.getByText('Add Knowledge Entry')).toBeInTheDocument();
    });

    it('renders wizard steps', () => {
      renderWikiPage();
      const addButton = screen.getAllByText('Add Knowledge')[0];
      fireEvent.click(addButton);
      expect(screen.getByText('Text Content')).toBeInTheDocument();
      expect(screen.getByText('URL / Web Page')).toBeInTheDocument();
    });

    it('closes wizard when Cancel button is clicked', async () => {
      renderWikiPage();
      const addButton = screen.getAllByText('Add Knowledge')[0];
      fireEvent.click(addButton);
      const cancelButton = screen.getByRole('button', { name: 'Cancel' });
      fireEvent.click(cancelButton);
      await waitFor(() => {
        expect(screen.queryByText('Add Knowledge Entry')).not.toBeInTheDocument();
      });
    });

    it('closes wizard when close icon is clicked', async () => {
      renderWikiPage();
      const addButton = screen.getAllByText('Add Knowledge')[0];
      fireEvent.click(addButton);
      const closeButton = screen.getByRole('button', { name: 'Close wizard' });
      fireEvent.click(closeButton);
      await waitFor(() => {
        expect(screen.queryByText('Add Knowledge Entry')).not.toBeInTheDocument();
      });
    });

    it('closes wizard when clicking overlay', async () => {
      renderWikiPage();
      const addButton = screen.getAllByText('Add Knowledge')[0];
      fireEvent.click(addButton);
      const overlay = document.querySelector('.ui-wizard-overlay');
      if (overlay) {
        fireEvent.click(overlay);
      }
      await waitFor(() => {
        expect(screen.queryByText('Add Knowledge Entry')).not.toBeInTheDocument();
      });
    });

    it('shows URL input when URL type is selected', async () => {
      renderWikiPage();
      const addButton = screen.getAllByText('Add Knowledge')[0];
      fireEvent.click(addButton);
      fireEvent.click(screen.getByText('URL / Web Page'));
      fireEvent.click(screen.getByRole('button', { name: 'Next' }));
      await waitFor(() => {
        expect(screen.getByLabelText('URL')).toBeInTheDocument();
      });
      expect((screen.getByLabelText('URL') as HTMLInputElement).type).toBe('url');
    });
  });

  describe.skip('Create Knowledge Submission', () => {
    it('creates text knowledge entry via wizard', async () => {
      const newEntry: KnowledgeEntry = {
        ...defaultEntries[0],
        id: 'kb-new',
        title: 'New Entry',
        content: 'New content',
      };
      mockCreateEntry.mockResolvedValueOnce(newEntry);
      renderWikiPage();
      const addButton = screen.getAllByText('Add Knowledge')[0];
      fireEvent.click(addButton);
      fireEvent.click(screen.getByRole('button', { name: 'Next' }));
      await waitFor(() => {
        expect(screen.getByLabelText(/Content/)).toBeInTheDocument();
      });
      const contentInput = screen.getByLabelText(/Content/);
      fireEvent.change(contentInput, { target: { value: 'New content' } });
      fireEvent.click(screen.getByRole('button', { name: 'Next' }));
      await waitFor(() => {
        expect(screen.getByLabelText('Title')).toBeInTheDocument();
      });
      const titleInput = screen.getByLabelText('Title');
      fireEvent.change(titleInput, { target: { value: 'New Entry' } });
      const submitButton = screen.getByRole('button', { name: 'Create Entry' });
      fireEvent.click(submitButton);
      await waitFor(() => {
        expect(mockCreateEntry).toHaveBeenCalledWith({
          title: 'New Entry',
          type: 'text',
          content: 'New content',
          tags: undefined,
        });
      });
    });

    it('creates URL knowledge entry via wizard', async () => {
      const newEntry: KnowledgeEntry = {
        ...defaultEntries[1],
        id: 'kb-new',
        title: 'New URL',
        content: 'https://newurl.com',
      };
      mockCreateEntry.mockResolvedValueOnce(newEntry);
      renderWikiPage();
      const addButton = screen.getAllByText('Add Knowledge')[0];
      fireEvent.click(addButton);
      fireEvent.click(screen.getByText('URL / Web Page'));
      fireEvent.click(screen.getByRole('button', { name: 'Next' }));
      await waitFor(() => {
        expect(screen.getByLabelText('URL')).toBeInTheDocument();
      });
      const urlInput = screen.getByLabelText('URL');
      fireEvent.change(urlInput, { target: { value: 'https://newurl.com' } });
      fireEvent.click(screen.getByRole('button', { name: 'Next' }));
      await waitFor(() => {
        expect(screen.getByLabelText('Title')).toBeInTheDocument();
      });
      const titleInput = screen.getByLabelText('Title');
      fireEvent.change(titleInput, { target: { value: 'New URL' } });
      const submitButton = screen.getByRole('button', { name: 'Create Entry' });
      fireEvent.click(submitButton);
      await waitFor(() => {
        expect(mockCreateEntry).toHaveBeenCalledWith({
          title: 'New URL',
          type: 'url',
          content: 'https://newurl.com',
          tags: undefined,
        });
      });
    });

    it('adds tags to knowledge entry via wizard', async () => {
      const newEntry: KnowledgeEntry = {
        ...defaultEntries[0],
        id: 'kb-new',
        tags: ['tag1', 'tag2'],
      };
      mockCreateEntry.mockResolvedValueOnce(newEntry);
      renderWikiPage();
      const addButton = screen.getAllByText('Add Knowledge')[0];
      fireEvent.click(addButton);
      fireEvent.click(screen.getByRole('button', { name: 'Next' }));
      await waitFor(() => {
        expect(screen.getByLabelText(/Content/)).toBeInTheDocument();
      });
      const contentInput = screen.getByLabelText(/Content/);
      fireEvent.change(contentInput, { target: { value: 'Content' } });
      fireEvent.click(screen.getByRole('button', { name: 'Next' }));
      await waitFor(() => {
        expect(screen.getByLabelText('Title')).toBeInTheDocument();
      });
      const titleInput = screen.getByLabelText('Title');
      fireEvent.change(titleInput, { target: { value: 'Tagged Entry' } });
      const tagsInput = screen.getByLabelText(/Tags/);
      fireEvent.change(tagsInput, { target: { value: 'tag1' } });
      fireEvent.keyDown(tagsInput, { key: 'Enter' });
      fireEvent.change(tagsInput, { target: { value: 'tag2' } });
      fireEvent.keyDown(tagsInput, { key: 'Enter' });
      const submitButton = screen.getByRole('button', { name: 'Create Entry' });
      fireEvent.click(submitButton);
      await waitFor(() => {
        expect(mockCreateEntry).toHaveBeenCalledWith(
          expect.objectContaining({
            tags: ['tag1', 'tag2'],
          })
        );
      });
    });

    it('removes tags when tag remove button is clicked', async () => {
      renderWikiPage();
      const addButton = screen.getAllByText('Add Knowledge')[0];
      fireEvent.click(addButton);
      fireEvent.click(screen.getByRole('button', { name: 'Next' }));
      await waitFor(() => {
        expect(screen.getByLabelText(/Content/)).toBeInTheDocument();
      });
      const contentInput = screen.getByLabelText(/Content/);
      fireEvent.change(contentInput, { target: { value: 'Content' } });
      fireEvent.click(screen.getByRole('button', { name: 'Next' }));
      await waitFor(() => {
        expect(screen.getByLabelText(/Tags/)).toBeInTheDocument();
      });
      const tagsInput = screen.getByLabelText(/Tags/);
      fireEvent.change(tagsInput, { target: { value: 'tag1' } });
      fireEvent.keyDown(tagsInput, { key: 'Enter' });
      await waitFor(() => {
        expect(screen.getByLabelText('Remove tag tag1')).toBeInTheDocument();
      });
      const removeButton = screen.getByLabelText('Remove tag tag1');
      fireEvent.click(removeButton);
      await waitFor(() => {
        expect(screen.queryByLabelText('Remove tag tag1')).not.toBeInTheDocument();
      });
    });

    it('wizard requires content before proceeding', async () => {
      renderWikiPage();
      const addButton = screen.getAllByText('Add Knowledge')[0];
      fireEvent.click(addButton);
      fireEvent.click(screen.getByRole('button', { name: 'Next' }));
      await waitFor(() => {
        expect(screen.getByLabelText(/Content/)).toBeInTheDocument();
      });
      const nextButton = screen.getByRole('button', { name: 'Next' });
      expect(nextButton).toBeDisabled();
    });

    it('displays error message on creation failure', async () => {
      mockCreateEntry.mockRejectedValueOnce(new Error('Creation failed'));
      renderWikiPage();
      const addButton = screen.getAllByText('Add Knowledge')[0];
      fireEvent.click(addButton);
      fireEvent.click(screen.getByRole('button', { name: 'Next' }));
      await waitFor(() => {
        expect(screen.getByLabelText(/Content/)).toBeInTheDocument();
      });
      const contentInput = screen.getByLabelText(/Content/);
      fireEvent.change(contentInput, { target: { value: 'New content' } });
      fireEvent.click(screen.getByRole('button', { name: 'Next' }));
      await waitFor(() => {
        expect(screen.getByLabelText('Title')).toBeInTheDocument();
      });
      const titleInput = screen.getByLabelText('Title');
      fireEvent.change(titleInput, { target: { value: 'New Entry' } });
      const submitButton = screen.getByRole('button', { name: 'Create Entry' });
      fireEvent.click(submitButton);
      await waitFor(() => {
        expect(screen.getByText('Creation failed')).toBeInTheDocument();
      });
    });

    it('shows submitting state during creation', async () => {
      mockCreateEntry.mockImplementationOnce(
        () => new Promise((resolve) => setTimeout(resolve, 100))
      );
      renderWikiPage();
      const addButton = screen.getAllByText('Add Knowledge')[0];
      fireEvent.click(addButton);
      fireEvent.click(screen.getByRole('button', { name: 'Next' }));
      await waitFor(() => {
        expect(screen.getByLabelText(/Content/)).toBeInTheDocument();
      });
      const contentInput = screen.getByLabelText(/Content/);
      fireEvent.change(contentInput, { target: { value: 'New content' } });
      fireEvent.click(screen.getByRole('button', { name: 'Next' }));
      await waitFor(() => {
        expect(screen.getByLabelText('Title')).toBeInTheDocument();
      });
      const titleInput = screen.getByLabelText('Title');
      fireEvent.change(titleInput, { target: { value: 'New Entry' } });
      const submitButton = screen.getByRole('button', { name: 'Create Entry' });
      fireEvent.click(submitButton);
      expect(screen.getByText('Creating...')).toBeInTheDocument();
      expect(submitButton).toBeDisabled();
    });
  });

  describe('Delete Knowledge', () => {
    beforeEach(() => {
      window.confirm = mock(() => true);
    });

    it('deletes knowledge entry when delete button is clicked', async () => {
      mockDeleteEntry.mockResolvedValueOnce(undefined);
      renderWikiPage();
      const deleteButtons = screen.getAllByLabelText('Delete entry');
      fireEvent.click(deleteButtons[0]);
      expect(window.confirm).toHaveBeenCalledWith(
        'Are you sure you want to delete this knowledge entry?'
      );
      await waitFor(() => {
        expect(mockDeleteEntry).toHaveBeenCalledWith('kb-1');
      });
    });

    it('does not delete when confirmation is cancelled', () => {
      window.confirm = mock(() => false);
      renderWikiPage();
      const deleteButtons = screen.getAllByLabelText('Delete entry');
      fireEvent.click(deleteButtons[0]);
      expect(mockDeleteEntry).not.toHaveBeenCalled();
    });

    it('displays error message on deletion failure', async () => {
      mockDeleteEntry.mockRejectedValueOnce(new Error('Deletion failed'));
      renderWikiPage();
      const deleteButtons = screen.getAllByLabelText('Delete entry');
      fireEvent.click(deleteButtons[0]);
      await waitFor(() => {
        expect(screen.getByText('Deletion failed')).toBeInTheDocument();
      });
    });
  });

  describe('Refresh Knowledge', () => {
    it('refreshes URL knowledge entry', async () => {
      const refreshedEntry = {
        ...defaultEntries[1],
        fetched_content: 'Updated fetched content',
        last_refreshed_at: '2024-01-03T00:00:00Z',
      };
      mockRefreshEntry.mockResolvedValueOnce(refreshedEntry);
      renderWikiPage();
      const refreshButton = screen.getByLabelText('Refresh URL content');
      fireEvent.click(refreshButton);
      await waitFor(() => {
        expect(mockRefreshEntry).toHaveBeenCalledWith('kb-2');
      });
    });

    it('displays error message on refresh failure', async () => {
      mockRefreshEntry.mockRejectedValueOnce(new Error('Refresh failed'));
      renderWikiPage();
      const refreshButton = screen.getByLabelText('Refresh URL content');
      fireEvent.click(refreshButton);
      await waitFor(() => {
        expect(screen.getByText('Refresh failed')).toBeInTheDocument();
      });
    });
  });

  describe('View Entry Modal', () => {
    it('opens view modal when entry card is clicked', () => {
      renderWikiPage();
      const entryCard = screen.getByText('Text Entry').closest('.knowledge-card');
      if (entryCard) {
        fireEvent.click(entryCard);
      }
      expect(screen.getAllByText('Text Entry').length).toBeGreaterThan(1);
    });

    it('displays full entry details in modal', () => {
      renderWikiPage();
      const entryCard = screen.getByText('URL Entry').closest('.knowledge-card');
      if (entryCard) {
        fireEvent.click(entryCard);
      }
      expect(screen.getAllByText('https://example.com').length).toBeGreaterThan(0);
      expect(screen.getAllByText('Fetched content from URL').length).toBeGreaterThan(1);
    });

    it('closes view modal when close button is clicked', async () => {
      renderWikiPage();
      const entryCard = screen.getByText('Text Entry').closest('.knowledge-card');
      if (entryCard) {
        fireEvent.click(entryCard);
      }
      expect(screen.getAllByText('Text Entry').length).toBeGreaterThan(1);
      const closeButtons = screen.getAllByLabelText('Close modal');
      fireEvent.click(closeButtons[closeButtons.length - 1]);
      await waitFor(() => {
        expect(screen.getAllByText('Text Entry').length).toBe(1);
      });
    });

    it('shows refresh button in view modal for URL entries', () => {
      renderWikiPage();
      const entryCard = screen.getByText('URL Entry').closest('.knowledge-card');
      if (entryCard) {
        fireEvent.click(entryCard);
      }
      expect(screen.getByRole('button', { name: 'Refresh Content' })).toBeInTheDocument();
    });

    it('does not show refresh button for text entries', () => {
      renderWikiPage();
      const entryCard = screen.getByText('Text Entry').closest('.knowledge-card');
      if (entryCard) {
        fireEvent.click(entryCard);
      }
      expect(screen.getAllByText('Text Entry').length).toBeGreaterThan(1);
      expect(screen.queryByRole('button', { name: 'Refresh Content' })).not.toBeInTheDocument();
    });
  });

  describe('Accessibility', () => {
    it('has proper ARIA labels for interactive elements', () => {
      renderWikiPage();
      expect(screen.getByLabelText('Search knowledge')).toBeInTheDocument();
      expect(screen.getAllByLabelText('Delete entry').length).toBeGreaterThan(0);
      expect(screen.getByLabelText('Refresh URL content')).toBeInTheDocument();
    });

    it('uses proper heading hierarchy', () => {
      renderWikiPage();
      expect(screen.getByRole('heading', { level: 1 })).toBeInTheDocument();
    });

    it.skip('includes role="alert" for error messages', () => {
      getMockState = () => ({ entries: [], loading: false, error: 'Load error' });
      renderWikiPage();
      const alert = screen.getByRole('alert');
      expect(alert).toHaveTextContent('Load error');
    });
  });
});
