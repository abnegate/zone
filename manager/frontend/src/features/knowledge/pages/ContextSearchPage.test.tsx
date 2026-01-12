import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterAll, beforeAll, beforeEach, describe, expect, it, mock } from 'bun:test';

const mockSearchContext = mock();
const mockGetSources = mock();

mock.module('../../../api/knowledge', () => ({
  knowledgeApi: {
    searchContext: mockSearchContext,
  },
}));

mock.module('../../../api/sources', () => ({
  sourcesApi: {
    getSources: mockGetSources,
  },
}));

let ContextSearchPage: typeof import('./ContextSearchPage').default;

beforeAll(async () => {
  ContextSearchPage = (await import('./ContextSearchPage')).default;
});

afterAll(() => {
  mock.restore();
});

describe('ContextSearchPage', () => {
  const mockSources = [
    {
      id: 's1',
      name: 'GitHub Repo',
      source_type: 'github' as const,
      category: 'file' as const,
      config: { owner: 'test', repo: 'repo' },
      description: null,
      url: 'https://github.com/test/repo',
      is_active: true,
      last_verified_at: null,
      last_error: null,
      created_at: '2024-01-01T00:00:00Z',
      updated_at: '2024-01-01T00:00:00Z',
    },
    {
      id: 's2',
      name: 'Local Files',
      source_type: 'filesystem' as const,
      category: 'file' as const,
      config: { base_path: '/data' },
      description: null,
      url: 'file:///data',
      is_active: true,
      last_verified_at: null,
      last_error: null,
      created_at: '2024-01-01T00:00:00Z',
      updated_at: '2024-01-01T00:00:00Z',
    },
  ];

  const mockResults = [
    {
      id: 'r1',
      source_id: 's1',
      source_name: 'GitHub Repo',
      content: 'Full content of the document',
      snippet: 'This is a test snippet with matching terms',
      relevance_score: 0.95,
      metadata: { type: 'file', path: '/src/test.ts' },
    },
    {
      id: 'r2',
      source_id: 's2',
      source_name: 'Local Files',
      content: 'Another document content',
      snippet: 'Another result snippet',
      relevance_score: 0.75,
      metadata: { type: 'file', path: '/docs/readme.md' },
    },
  ];

  beforeEach(() => {
    jest.clearAllMocks();
    mockGetSources.mockResolvedValue(mockSources);
  });

  it('should render the search page', async () => {
    render(<ContextSearchPage />);

    expect(screen.getByText('Context Search')).toBeInTheDocument();
    expect(screen.getByPlaceholderText('Search your context...')).toBeInTheDocument();
    expect(screen.getByText('Search')).toBeInTheDocument();

    await waitFor(() => {
      expect(mockGetSources).toHaveBeenCalledWith(undefined, true);
    });
  });

  it('should load and display sources', async () => {
    render(<ContextSearchPage />);

    await waitFor(() => {
      expect(screen.getByText('GitHub Repo')).toBeInTheDocument();
      expect(screen.getByText('Local Files')).toBeInTheDocument();
    });
  });

  it('should perform search with query', async () => {
    mockSearchContext.mockResolvedValue({
      results: mockResults,
      total: 2,
    });

    render(<ContextSearchPage />);

    const input = screen.getByPlaceholderText('Search your context...');
    const searchButton = screen.getByText('Search');

    fireEvent.change(input, { target: { value: 'test query' } });
    fireEvent.click(searchButton);

    await waitFor(() => {
      expect(mockSearchContext).toHaveBeenCalledWith({
        query: 'test query',
        mode: 'hybrid',
        source_ids: undefined,
        limit: 20,
      });
    });

    await waitFor(() => {
      expect(screen.getByText('2 results found')).toBeInTheDocument();
    });

    // Verify result items are rendered
    const resultItems = document.querySelectorAll('.result-item');
    expect(resultItems.length).toBe(2);

    // Verify snippets are rendered (they use dangerouslySetInnerHTML)
    const snippets = document.querySelectorAll('.result-snippet');
    expect(snippets.length).toBe(2);
  });

  it('should change search mode', async () => {
    mockSearchContext.mockResolvedValue({ results: [], total: 0 });

    render(<ContextSearchPage />);

    const semanticButton = screen.getByText('Semantic');
    fireEvent.click(semanticButton);

    expect(semanticButton).toHaveClass('active');

    const input = screen.getByPlaceholderText('Search your context...');
    fireEvent.change(input, { target: { value: 'test' } });
    fireEvent.submit(input.closest('form')!);

    await waitFor(() => {
      expect(mockSearchContext).toHaveBeenCalledWith(
        expect.objectContaining({ mode: 'semantic' })
      );
    });
  });

  it('should filter by selected sources', async () => {
    mockSearchContext.mockResolvedValue({ results: [], total: 0 });

    render(<ContextSearchPage />);

    await waitFor(() => {
      expect(screen.getByText('GitHub Repo')).toBeInTheDocument();
    });

    const checkbox = screen.getByLabelText('GitHub Repo') as HTMLInputElement;
    fireEvent.click(checkbox);

    expect(checkbox.checked).toBe(true);

    const input = screen.getByPlaceholderText('Search your context...');
    fireEvent.change(input, { target: { value: 'test' } });
    fireEvent.submit(input.closest('form')!);

    await waitFor(() => {
      expect(mockSearchContext).toHaveBeenCalledWith(
        expect.objectContaining({ source_ids: ['s1'] })
      );
    });
  });

  it('should display relevance scores', async () => {
    mockSearchContext.mockResolvedValue({
      results: mockResults,
      total: 2,
    });

    render(<ContextSearchPage />);

    const input = screen.getByPlaceholderText('Search your context...');
    fireEvent.change(input, { target: { value: 'test' } });
    fireEvent.submit(input.closest('form')!);

    await waitFor(() => {
      expect(screen.getByText('95% relevant')).toBeInTheDocument();
      expect(screen.getByText('75% relevant')).toBeInTheDocument();
    });
  });

  it('should display file metadata', async () => {
    mockSearchContext.mockResolvedValue({
      results: mockResults,
      total: 2,
    });

    render(<ContextSearchPage />);

    const input = screen.getByPlaceholderText('Search your context...');
    fireEvent.change(input, { target: { value: 'test' } });
    fireEvent.submit(input.closest('form')!);

    await waitFor(() => {
      expect(screen.getByText('/src/test.ts')).toBeInTheDocument();
      expect(screen.getByText('/docs/readme.md')).toBeInTheDocument();
    });
  });

  it('should show empty state when no results', async () => {
    mockSearchContext.mockResolvedValue({ results: [], total: 0 });

    render(<ContextSearchPage />);

    const input = screen.getByPlaceholderText('Search your context...');
    fireEvent.change(input, { target: { value: 'nonexistent' } });
    fireEvent.submit(input.closest('form')!);

    await waitFor(() => {
      expect(screen.getByText('No results found')).toBeInTheDocument();
      expect(screen.getByText('Try adjusting your search query or filters')).toBeInTheDocument();
    });
  });

  it('should show error message on search failure', async () => {
    mockSearchContext.mockRejectedValue(new Error('Search failed'));

    render(<ContextSearchPage />);

    const input = screen.getByPlaceholderText('Search your context...');
    fireEvent.change(input, { target: { value: 'test' } });
    fireEvent.submit(input.closest('form')!);

    await waitFor(() => {
      expect(screen.getByRole('alert')).toHaveTextContent('Search failed');
    });
  });

  it('should disable search button when query is empty', () => {
    render(<ContextSearchPage />);

    const searchButton = screen.getByText('Search') as HTMLButtonElement;
    expect(searchButton.disabled).toBe(true);
  });

  it('should show loading state during search', async () => {
    mockSearchContext.mockImplementation(
      () => new Promise((resolve) => setTimeout(() => resolve({ results: [], total: 0 }), 100))
    );

    render(<ContextSearchPage />);

    const input = screen.getByPlaceholderText('Search your context...');
    fireEvent.change(input, { target: { value: 'test' } });
    fireEvent.submit(input.closest('form')!);

    expect(screen.getByText('Search').querySelector('.spinner')).toBeInTheDocument();

    await waitFor(() => {
      expect(screen.queryByText('Search')?.querySelector('.spinner')).not.toBeInTheDocument();
    });
  });

  it('should display initial empty state', () => {
    render(<ContextSearchPage />);

    expect(screen.getByText('Start searching')).toBeInTheDocument();
    expect(
      screen.getByText('Enter a query to search across your connected sources')
    ).toBeInTheDocument();
  });

  it('should trim whitespace from query', async () => {
    mockSearchContext.mockResolvedValue({ results: [], total: 0 });

    render(<ContextSearchPage />);

    const input = screen.getByPlaceholderText('Search your context...');
    fireEvent.change(input, { target: { value: '  test query  ' } });
    fireEvent.submit(input.closest('form')!);

    await waitFor(() => {
      expect(mockSearchContext).toHaveBeenCalledWith(
        expect.objectContaining({ query: 'test query' })
      );
    });
  });

  it('should toggle source selection', async () => {
    render(<ContextSearchPage />);

    await waitFor(() => {
      expect(screen.getByText('GitHub Repo')).toBeInTheDocument();
    });

    const checkbox = screen.getByLabelText('GitHub Repo') as HTMLInputElement;

    fireEvent.click(checkbox);
    expect(checkbox.checked).toBe(true);

    fireEvent.click(checkbox);
    expect(checkbox.checked).toBe(false);
  });

  it('should handle no sources available', async () => {
    mockGetSources.mockResolvedValue([]);

    render(<ContextSearchPage />);

    await waitFor(() => {
      expect(screen.getByText('No sources available')).toBeInTheDocument();
    });
  });

  it('should sanitize highlighted snippets to prevent XSS', async () => {
    const xssResults = [
      {
        id: 'r1',
        source_id: 's1',
        source_name: 'Test Source',
        content: 'Content with XSS attempt',
        snippet: 'Text with <script>alert("xss")</script> malicious code',
        relevance_score: 0.9,
        metadata: {},
      },
    ];

    mockSearchContext.mockResolvedValue({
      results: xssResults,
      total: 1,
    });

    render(<ContextSearchPage />);

    const input = screen.getByPlaceholderText('Search your context...');
    fireEvent.change(input, { target: { value: 'test' } });
    fireEvent.submit(input.closest('form')!);

    await waitFor(() => {
      const snippet = document.querySelector('.result-snippet');
      expect(snippet).toBeInTheDocument();
      // Script tag should be sanitized out by DOMPurify
      expect(snippet?.innerHTML).not.toContain('<script>');
    });
  });

  it('should escape regex special characters in search query', async () => {
    const regexResults = [
      {
        id: 'r1',
        source_id: 's1',
        source_name: 'Test Source',
        content: 'Test content',
        snippet: 'Test with special chars (.*+?)',
        relevance_score: 0.9,
        metadata: {},
      },
    ];

    mockSearchContext.mockResolvedValue({
      results: regexResults,
      total: 1,
    });

    render(<ContextSearchPage />);

    const input = screen.getByPlaceholderText('Search your context...');
    // Search query with special regex characters
    fireEvent.change(input, { target: { value: '(.*+?)' } });
    fireEvent.submit(input.closest('form')!);

    await waitFor(() => {
      const snippet = document.querySelector('.result-snippet');
      expect(snippet).toBeInTheDocument();
      // Should not throw regex error, characters should be escaped
      expect(snippet?.textContent).toContain('(.*+?)');
    });
  });
});
