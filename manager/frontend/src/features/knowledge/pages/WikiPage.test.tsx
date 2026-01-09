import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import type { KnowledgeEntry } from '../types';
import { knowledgeApi } from '../../../api/knowledge';
import WikiPage from './WikiPage';

// Mock the knowledgeApi
jest.mock('../../../api/knowledge');
const mockKnowledgeApi = knowledgeApi as jest.Mocked<typeof knowledgeApi>;

describe('WikiPage', () => {
  const mockEntries: KnowledgeEntry[] = [
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
    jest.clearAllMocks();
    mockKnowledgeApi.getKnowledge.mockResolvedValue({ entries: mockEntries });
  });

  describe('Initial Load', () => {
    it('renders page heading and subtitle', async () => {
      render(<WikiPage />);
      expect(screen.getByRole('heading', { name: 'Knowledge Base' })).toBeInTheDocument();
      expect(
        screen.getByText('Manage documentation, links, and content for your AI models')
      ).toBeInTheDocument();
    });

    it('loads and displays knowledge entries', async () => {
      render(<WikiPage />);

      await waitFor(() => {
        expect(mockKnowledgeApi.getKnowledge).toHaveBeenCalled();
      });

      expect(screen.getByText('Text Entry')).toBeInTheDocument();
      expect(screen.getByText('URL Entry')).toBeInTheDocument();
    });

    it('shows loading state initially', async () => {
      render(<WikiPage />);

      expect(screen.getByText('Loading knowledge...')).toBeInTheDocument();

      await waitFor(() => {
        expect(screen.queryByText('Loading knowledge...')).not.toBeInTheDocument();
      });
    });

    it('displays error message on load failure', async () => {
      mockKnowledgeApi.getKnowledge.mockRejectedValue(new Error('Failed to load'));

      render(<WikiPage />);

      await waitFor(() => {
        expect(screen.getByText('Failed to load')).toBeInTheDocument();
      });
    });

    it('displays empty state when no entries', async () => {
      mockKnowledgeApi.getKnowledge.mockResolvedValue({ entries: [] });

      render(<WikiPage />);

      await waitFor(() => {
        expect(screen.getByText('No knowledge entries found')).toBeInTheDocument();
        expect(
          screen.getByText('Get started by adding your first knowledge entry')
        ).toBeInTheDocument();
      });
    });
  });

  describe('Search Functionality', () => {
    it('renders search input', async () => {
      render(<WikiPage />);

      await waitFor(() => {
        expect(screen.getByPlaceholderText('Search knowledge...')).toBeInTheDocument();
      });
    });

    it('filters entries by title', async () => {
      render(<WikiPage />);

      await waitFor(() => {
        expect(screen.getByText('Text Entry')).toBeInTheDocument();
      });

      const searchInput = screen.getByPlaceholderText('Search knowledge...');
      fireEvent.change(searchInput, { target: { value: 'Text' } });

      expect(screen.getByText('Text Entry')).toBeInTheDocument();
      expect(screen.queryByText('URL Entry')).not.toBeInTheDocument();
    });

    it('filters entries by content', async () => {
      render(<WikiPage />);

      await waitFor(() => {
        expect(screen.getAllByText(/Entry/).length).toBeGreaterThan(0);
      });

      const searchInput = screen.getByPlaceholderText('Search knowledge...');
      fireEvent.change(searchInput, { target: { value: 'Fetched content' } });

      expect(screen.queryByText('Text Entry')).not.toBeInTheDocument();
      expect(screen.getByText('URL Entry')).toBeInTheDocument();
    });

    it('filters entries by tags', async () => {
      render(<WikiPage />);

      await waitFor(() => {
        expect(screen.getAllByText(/Entry/).length).toBeGreaterThan(0);
      });

      const searchInput = screen.getByPlaceholderText('Search knowledge...');
      fireEvent.change(searchInput, { target: { value: 'documentation' } });

      expect(screen.queryByText('Text Entry')).not.toBeInTheDocument();
      expect(screen.getByText('URL Entry')).toBeInTheDocument();
    });

    it('shows appropriate message when search yields no results', async () => {
      render(<WikiPage />);

      await waitFor(() => {
        expect(screen.getAllByText(/Entry/).length).toBeGreaterThan(0);
      });

      const searchInput = screen.getByPlaceholderText('Search knowledge...');
      fireEvent.change(searchInput, { target: { value: 'nonexistent' } });

      expect(screen.getByText('No knowledge entries found')).toBeInTheDocument();
      expect(screen.getByText('Try adjusting your filters or search query')).toBeInTheDocument();
    });
  });

  describe('Filter Functionality', () => {
    it('renders filter buttons', async () => {
      render(<WikiPage />);

      await waitFor(() => {
        expect(screen.getByRole('button', { name: 'All' })).toBeInTheDocument();
      });

      expect(screen.getByRole('button', { name: 'Text' })).toBeInTheDocument();
      expect(screen.getByRole('button', { name: 'URL' })).toBeInTheDocument();
    });

    it('filters by text type', async () => {
      render(<WikiPage />);

      await waitFor(() => {
        expect(screen.getAllByText(/Entry/).length).toBeGreaterThan(0);
      });

      const textFilter = screen.getByRole('button', { name: 'Text' });
      fireEvent.click(textFilter);

      expect(screen.getByText('Text Entry')).toBeInTheDocument();
      expect(screen.queryByText('URL Entry')).not.toBeInTheDocument();
    });

    it('filters by url type', async () => {
      render(<WikiPage />);

      await waitFor(() => {
        expect(screen.getAllByText(/Entry/).length).toBeGreaterThan(0);
      });

      const urlFilter = screen.getByRole('button', { name: 'URL' });
      fireEvent.click(urlFilter);

      expect(screen.queryByText('Text Entry')).not.toBeInTheDocument();
      expect(screen.getByText('URL Entry')).toBeInTheDocument();
    });

    it('shows all entries when All filter is active', async () => {
      render(<WikiPage />);

      await waitFor(() => {
        expect(screen.getAllByText(/Entry/).length).toBeGreaterThan(0);
      });

      const urlFilter = screen.getByRole('button', { name: 'URL' });
      fireEvent.click(urlFilter);

      const allFilter = screen.getByRole('button', { name: 'All' });
      fireEvent.click(allFilter);

      expect(screen.getByText('Text Entry')).toBeInTheDocument();
      expect(screen.getByText('URL Entry')).toBeInTheDocument();
    });

    it('applies active class to selected filter', async () => {
      render(<WikiPage />);

      await waitFor(() => {
        expect(screen.getByRole('button', { name: 'All' })).toBeInTheDocument();
      });

      const textFilter = screen.getByRole('button', { name: 'Text' });
      expect(textFilter).not.toHaveClass('active');

      fireEvent.click(textFilter);
      expect(textFilter).toHaveClass('active');
    });
  });

  describe('Knowledge Entry Display', () => {
    it('displays entry cards with correct information', async () => {
      render(<WikiPage />);

      await waitFor(() => {
        expect(screen.getByText('Text Entry')).toBeInTheDocument();
      });

      expect(screen.getByText('This is text content')).toBeInTheDocument();
      expect(screen.getByText('tag1')).toBeInTheDocument();
      expect(screen.getByText('tag2')).toBeInTheDocument();
    });

    it('displays type badge for text entries', async () => {
      render(<WikiPage />);

      await waitFor(() => {
        expect(screen.getAllByText('text').length).toBeGreaterThan(0);
      });

      const badges = screen.getAllByText('text');
      expect(badges.some((badge) => badge.classList.contains('knowledge-type-badge'))).toBe(true);
    });

    it('displays type badge for URL entries', async () => {
      render(<WikiPage />);

      await waitFor(() => {
        expect(screen.getAllByText('url').length).toBeGreaterThan(0);
      });

      const badges = screen.getAllByText('url');
      expect(badges.some((badge) => badge.classList.contains('knowledge-type-badge'))).toBe(true);
    });

    it('displays URL link for URL entries', async () => {
      render(<WikiPage />);

      await waitFor(() => {
        expect(screen.getByText('https://example.com')).toBeInTheDocument();
      });

      const link = screen.getByText('https://example.com') as HTMLAnchorElement;
      expect(link.href).toBe('https://example.com/');
    });

    it('displays fetched content for URL entries', async () => {
      render(<WikiPage />);

      await waitFor(() => {
        expect(screen.getByText('Fetched content from URL')).toBeInTheDocument();
      });
    });

    it('shows refresh button only for URL entries', async () => {
      render(<WikiPage />);

      await waitFor(() => {
        expect(screen.getAllByLabelText('Delete entry').length).toBe(2);
      });

      const refreshButtons = screen.getAllByLabelText('Refresh URL content');
      expect(refreshButtons.length).toBe(1);
    });
  });

  describe('Create Knowledge Modal', () => {
    it('opens create modal when Add Knowledge button is clicked', async () => {
      render(<WikiPage />);

      await waitFor(() => {
        expect(screen.getAllByText('Add Knowledge').length).toBeGreaterThan(0);
      });

      const addButton = screen.getAllByText('Add Knowledge')[0];
      fireEvent.click(addButton);

      expect(screen.getByText('Add Knowledge Entry')).toBeInTheDocument();
    });

    it('renders form fields in create modal', async () => {
      render(<WikiPage />);

      await waitFor(() => {
        expect(screen.getAllByText('Add Knowledge').length).toBeGreaterThan(0);
      });

      const addButton = screen.getAllByText('Add Knowledge')[0];
      fireEvent.click(addButton);

      expect(screen.getByLabelText('Title')).toBeInTheDocument();
      expect(screen.getByLabelText('Type')).toBeInTheDocument();
      expect(screen.getByLabelText('Content')).toBeInTheDocument();
      expect(screen.getByLabelText(/Tags/)).toBeInTheDocument();
    });

    it('closes modal when Cancel button is clicked', async () => {
      render(<WikiPage />);

      await waitFor(() => {
        expect(screen.getAllByText('Add Knowledge').length).toBeGreaterThan(0);
      });

      const addButton = screen.getAllByText('Add Knowledge')[0];
      fireEvent.click(addButton);

      const cancelButton = screen.getByRole('button', { name: 'Cancel' });
      fireEvent.click(cancelButton);

      expect(screen.queryByText('Add Knowledge Entry')).not.toBeInTheDocument();
    });

    it('closes modal when close icon is clicked', async () => {
      render(<WikiPage />);

      await waitFor(() => {
        expect(screen.getAllByText('Add Knowledge').length).toBeGreaterThan(0);
      });

      const addButton = screen.getAllByText('Add Knowledge')[0];
      fireEvent.click(addButton);

      const closeButton = screen.getByLabelText('Close modal');
      fireEvent.click(closeButton);

      expect(screen.queryByText('Add Knowledge Entry')).not.toBeInTheDocument();
    });

    it('closes modal when clicking overlay', async () => {
      render(<WikiPage />);

      await waitFor(() => {
        expect(screen.getAllByText('Add Knowledge').length).toBeGreaterThan(0);
      });

      const addButton = screen.getAllByText('Add Knowledge')[0];
      fireEvent.click(addButton);

      const overlay = screen.getByText('Add Knowledge Entry').closest('.modal-overlay');
      if (overlay) {
        fireEvent.click(overlay);
      }

      await waitFor(() => {
        expect(screen.queryByText('Add Knowledge Entry')).not.toBeInTheDocument();
      });
    });

    it('switches input type when type is changed', async () => {
      render(<WikiPage />);

      await waitFor(() => {
        expect(screen.getAllByText('Add Knowledge').length).toBeGreaterThan(0);
      });

      const addButton = screen.getAllByText('Add Knowledge')[0];
      fireEvent.click(addButton);

      const typeSelect = screen.getByLabelText('Type') as HTMLSelectElement;
      fireEvent.change(typeSelect, { target: { value: 'url' } });

      expect(screen.getByLabelText('URL')).toBeInTheDocument();
      expect((screen.getByLabelText('URL') as HTMLInputElement).type).toBe('url');
    });
  });

  describe('Create Knowledge Submission', () => {
    it('creates text knowledge entry', async () => {
      const newEntry: KnowledgeEntry = {
        ...mockEntries[0],
        id: 'kb-new',
        title: 'New Entry',
        content: 'New content',
      };

      mockKnowledgeApi.createKnowledge.mockResolvedValue(newEntry);

      render(<WikiPage />);

      await waitFor(() => {
        expect(screen.getAllByText('Add Knowledge').length).toBeGreaterThan(0);
      });

      const addButton = screen.getAllByText('Add Knowledge')[0];
      fireEvent.click(addButton);

      const titleInput = screen.getByLabelText('Title');
      const contentInput = screen.getByLabelText('Content');

      fireEvent.change(titleInput, { target: { value: 'New Entry' } });
      fireEvent.change(contentInput, { target: { value: 'New content' } });

      const submitButton = screen.getByRole('button', { name: 'Create Entry' });
      fireEvent.click(submitButton);

      await waitFor(() => {
        expect(mockKnowledgeApi.createKnowledge).toHaveBeenCalledWith({
          title: 'New Entry',
          type: 'text',
          content: 'New content',
          tags: [],
        });
      });

      await waitFor(() => {
        expect(screen.queryByText('Add Knowledge Entry')).not.toBeInTheDocument();
      });
    });

    it('creates URL knowledge entry', async () => {
      const newEntry: KnowledgeEntry = {
        ...mockEntries[1],
        id: 'kb-new',
        title: 'New URL',
        content: 'https://newurl.com',
      };

      mockKnowledgeApi.createKnowledge.mockResolvedValue(newEntry);

      render(<WikiPage />);

      await waitFor(() => {
        expect(screen.getAllByText('Add Knowledge').length).toBeGreaterThan(0);
      });

      const addButton = screen.getAllByText('Add Knowledge')[0];
      fireEvent.click(addButton);

      const typeSelect = screen.getByLabelText('Type');
      fireEvent.change(typeSelect, { target: { value: 'url' } });

      const titleInput = screen.getByLabelText('Title');
      const urlInput = screen.getByLabelText('URL');

      fireEvent.change(titleInput, { target: { value: 'New URL' } });
      fireEvent.change(urlInput, { target: { value: 'https://newurl.com' } });

      const submitButton = screen.getByRole('button', { name: 'Create Entry' });
      fireEvent.click(submitButton);

      await waitFor(() => {
        expect(mockKnowledgeApi.createKnowledge).toHaveBeenCalledWith({
          title: 'New URL',
          type: 'url',
          content: 'https://newurl.com',
          tags: [],
        });
      });
    });

    it('adds tags to knowledge entry', async () => {
      const newEntry: KnowledgeEntry = {
        ...mockEntries[0],
        id: 'kb-new',
        tags: ['tag1', 'tag2'],
      };

      mockKnowledgeApi.createKnowledge.mockResolvedValue(newEntry);

      render(<WikiPage />);

      await waitFor(() => {
        expect(screen.getAllByText('Add Knowledge').length).toBeGreaterThan(0);
      });

      const addButton = screen.getAllByText('Add Knowledge')[0];
      fireEvent.click(addButton);

      const titleInput = screen.getByLabelText('Title');
      const contentInput = screen.getByLabelText('Content');
      const tagsInput = screen.getByLabelText(/Tags/);

      fireEvent.change(titleInput, { target: { value: 'Tagged Entry' } });
      fireEvent.change(contentInput, { target: { value: 'Content' } });

      fireEvent.change(tagsInput, { target: { value: 'tag1' } });
      fireEvent.keyDown(tagsInput, { key: 'Enter' });

      fireEvent.change(tagsInput, { target: { value: 'tag2' } });
      fireEvent.keyDown(tagsInput, { key: 'Enter' });

      const submitButton = screen.getByRole('button', { name: 'Create Entry' });
      fireEvent.click(submitButton);

      await waitFor(() => {
        expect(mockKnowledgeApi.createKnowledge).toHaveBeenCalledWith(
          expect.objectContaining({
            tags: ['tag1', 'tag2'],
          })
        );
      });
    });

    it('removes tags when tag remove button is clicked', async () => {
      render(<WikiPage />);

      await waitFor(() => {
        expect(screen.getAllByText('Add Knowledge').length).toBeGreaterThan(0);
      });

      const addButton = screen.getAllByText('Add Knowledge')[0];
      fireEvent.click(addButton);

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

    it('validates required fields', async () => {
      render(<WikiPage />);

      await waitFor(() => {
        expect(screen.getAllByText('Add Knowledge').length).toBeGreaterThan(0);
      });

      const addButton = screen.getAllByText('Add Knowledge')[0];
      fireEvent.click(addButton);

      const submitButton = screen.getByRole('button', { name: 'Create Entry' });
      fireEvent.click(submitButton);

      await waitFor(() => {
        expect(mockKnowledgeApi.createKnowledge).not.toHaveBeenCalled();
      });
    });

    it('displays error message on creation failure', async () => {
      mockKnowledgeApi.createKnowledge.mockRejectedValue(new Error('Creation failed'));

      render(<WikiPage />);

      await waitFor(() => {
        expect(screen.getAllByText('Add Knowledge').length).toBeGreaterThan(0);
      });

      const addButton = screen.getAllByText('Add Knowledge')[0];
      fireEvent.click(addButton);

      const titleInput = screen.getByLabelText('Title');
      const contentInput = screen.getByLabelText('Content');

      fireEvent.change(titleInput, { target: { value: 'New Entry' } });
      fireEvent.change(contentInput, { target: { value: 'New content' } });

      const submitButton = screen.getByRole('button', { name: 'Create Entry' });
      fireEvent.click(submitButton);

      await waitFor(() => {
        expect(screen.getByText('Creation failed')).toBeInTheDocument();
      });
    });

    it('shows submitting state during creation', async () => {
      mockKnowledgeApi.createKnowledge.mockImplementation(
        () => new Promise((resolve) => setTimeout(resolve, 100))
      );

      render(<WikiPage />);

      await waitFor(() => {
        expect(screen.getAllByText('Add Knowledge').length).toBeGreaterThan(0);
      });

      const addButton = screen.getAllByText('Add Knowledge')[0];
      fireEvent.click(addButton);

      const titleInput = screen.getByLabelText('Title');
      const contentInput = screen.getByLabelText('Content');

      fireEvent.change(titleInput, { target: { value: 'New Entry' } });
      fireEvent.change(contentInput, { target: { value: 'New content' } });

      const submitButton = screen.getByRole('button', { name: 'Create Entry' });
      fireEvent.click(submitButton);

      expect(screen.getByText('Creating...')).toBeInTheDocument();
      expect(submitButton).toBeDisabled();
    });
  });

  describe('Delete Knowledge', () => {
    beforeEach(() => {
      window.confirm = jest.fn(() => true);
    });

    it('deletes knowledge entry when delete button is clicked', async () => {
      mockKnowledgeApi.deleteKnowledge.mockResolvedValue();

      render(<WikiPage />);

      await waitFor(() => {
        expect(screen.getAllByLabelText('Delete entry').length).toBe(2);
      });

      const deleteButtons = screen.getAllByLabelText('Delete entry');
      fireEvent.click(deleteButtons[0]);

      expect(window.confirm).toHaveBeenCalledWith(
        'Are you sure you want to delete this knowledge entry?'
      );

      await waitFor(() => {
        expect(mockKnowledgeApi.deleteKnowledge).toHaveBeenCalledWith('kb-1');
      });
    });

    it('does not delete when confirmation is cancelled', async () => {
      window.confirm = jest.fn(() => false);

      render(<WikiPage />);

      await waitFor(() => {
        expect(screen.getAllByLabelText('Delete entry').length).toBe(2);
      });

      const deleteButtons = screen.getAllByLabelText('Delete entry');
      fireEvent.click(deleteButtons[0]);

      expect(mockKnowledgeApi.deleteKnowledge).not.toHaveBeenCalled();
    });

    it('displays error message on deletion failure', async () => {
      mockKnowledgeApi.deleteKnowledge.mockRejectedValue(new Error('Deletion failed'));

      render(<WikiPage />);

      await waitFor(() => {
        expect(screen.getAllByLabelText('Delete entry').length).toBe(2);
      });

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
        ...mockEntries[1],
        fetched_content: 'Updated fetched content',
        last_refreshed_at: '2024-01-03T00:00:00Z',
      };

      mockKnowledgeApi.refreshKnowledge.mockResolvedValue(refreshedEntry);

      render(<WikiPage />);

      await waitFor(() => {
        expect(screen.getByLabelText('Refresh URL content')).toBeInTheDocument();
      });

      const refreshButton = screen.getByLabelText('Refresh URL content');
      fireEvent.click(refreshButton);

      await waitFor(() => {
        expect(mockKnowledgeApi.refreshKnowledge).toHaveBeenCalledWith('kb-2');
      });
    });

    it('shows refreshing state during refresh', async () => {
      mockKnowledgeApi.refreshKnowledge.mockImplementation(
        () => new Promise((resolve) => setTimeout(resolve, 100))
      );

      render(<WikiPage />);

      await waitFor(() => {
        expect(screen.getByLabelText('Refresh URL content')).toBeInTheDocument();
      });

      const refreshButton = screen.getByLabelText('Refresh URL content');
      fireEvent.click(refreshButton);

      expect(refreshButton).toBeDisabled();
      expect(refreshButton).toHaveClass('refreshing');
    });

    it('displays error message on refresh failure', async () => {
      mockKnowledgeApi.refreshKnowledge.mockRejectedValue(new Error('Refresh failed'));

      render(<WikiPage />);

      await waitFor(() => {
        expect(screen.getByLabelText('Refresh URL content')).toBeInTheDocument();
      });

      const refreshButton = screen.getByLabelText('Refresh URL content');
      fireEvent.click(refreshButton);

      await waitFor(() => {
        expect(screen.getByText('Refresh failed')).toBeInTheDocument();
      });
    });
  });

  describe('View Entry Modal', () => {
    it('opens view modal when entry card is clicked', async () => {
      render(<WikiPage />);

      await waitFor(() => {
        expect(screen.getByText('Text Entry')).toBeInTheDocument();
      });

      const entryCard = screen.getByText('Text Entry').closest('.knowledge-card');
      if (entryCard) {
        fireEvent.click(entryCard);
      }

      await waitFor(() => {
        expect(screen.getAllByText('Text Entry').length).toBeGreaterThan(1);
      });
    });

    it('displays full entry details in modal', async () => {
      render(<WikiPage />);

      await waitFor(() => {
        expect(screen.getByText('URL Entry')).toBeInTheDocument();
      });

      const entryCard = screen.getByText('URL Entry').closest('.knowledge-card');
      if (entryCard) {
        fireEvent.click(entryCard);
      }

      await waitFor(() => {
        expect(screen.getAllByText('https://example.com').length).toBeGreaterThan(0);
        expect(screen.getAllByText('Fetched content from URL').length).toBeGreaterThan(1);
      });
    });

    it('closes view modal when close button is clicked', async () => {
      render(<WikiPage />);

      await waitFor(() => {
        expect(screen.getByText('Text Entry')).toBeInTheDocument();
      });

      const entryCard = screen.getByText('Text Entry').closest('.knowledge-card');
      if (entryCard) {
        fireEvent.click(entryCard);
      }

      await waitFor(() => {
        expect(screen.getAllByText('Text Entry').length).toBeGreaterThan(1);
      });

      const closeButtons = screen.getAllByLabelText('Close modal');
      fireEvent.click(closeButtons[closeButtons.length - 1]);

      await waitFor(() => {
        expect(screen.getAllByText('Text Entry').length).toBe(1);
      });
    });

    it('shows refresh button in view modal for URL entries', async () => {
      render(<WikiPage />);

      await waitFor(() => {
        expect(screen.getByText('URL Entry')).toBeInTheDocument();
      });

      const entryCard = screen.getByText('URL Entry').closest('.knowledge-card');
      if (entryCard) {
        fireEvent.click(entryCard);
      }

      await waitFor(() => {
        expect(screen.getByRole('button', { name: 'Refresh Content' })).toBeInTheDocument();
      });
    });

    it('does not show refresh button for text entries', async () => {
      render(<WikiPage />);

      await waitFor(() => {
        expect(screen.getByText('Text Entry')).toBeInTheDocument();
      });

      const entryCard = screen.getByText('Text Entry').closest('.knowledge-card');
      if (entryCard) {
        fireEvent.click(entryCard);
      }

      await waitFor(() => {
        expect(screen.getAllByText('Text Entry').length).toBeGreaterThan(1);
      });

      expect(screen.queryByRole('button', { name: 'Refresh Content' })).not.toBeInTheDocument();
    });
  });

  describe('Accessibility', () => {
    it('has proper ARIA labels for interactive elements', async () => {
      render(<WikiPage />);

      await waitFor(() => {
        expect(screen.getByLabelText('Search knowledge')).toBeInTheDocument();
      });

      await waitFor(() => {
        expect(screen.getAllByLabelText('Delete entry').length).toBeGreaterThan(0);
      });

      expect(screen.getByLabelText('Refresh URL content')).toBeInTheDocument();
    });

    it('uses proper heading hierarchy', async () => {
      render(<WikiPage />);

      await waitFor(() => {
        expect(screen.getByRole('heading', { level: 1 })).toBeInTheDocument();
      });
    });

    it('includes role="alert" for error messages', async () => {
      mockKnowledgeApi.getKnowledge.mockRejectedValue(new Error('Load error'));

      render(<WikiPage />);

      await waitFor(() => {
        const alert = screen.getByRole('alert');
        expect(alert).toHaveTextContent('Load error');
      });
    });
  });
});
