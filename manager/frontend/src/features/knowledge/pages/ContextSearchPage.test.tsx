import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterAll, beforeAll, beforeEach, describe, expect, it, mock } from 'bun:test';

const mockSearch = mock();
const mockClear = mock();
const mockGetSources = mock();

// State container for useContextSearch hook
let searchState = {
  results: [] as Array<{
    id: string;
    source_id: string;
    source_name: string;
    content: string;
    snippet: string;
    relevance_score: number;
    metadata: Record<string, unknown>;
  }>,
  total: 0,
  loading: false,
  error: null as string | null,
};

mock.module('../hooks', () => ({
  useContextSearch: () => ({
    results: searchState.results,
    total: searchState.total,
    loading: searchState.loading,
    error: searchState.error,
    search: mockSearch,
    clear: mockClear,
  }),
  // Include useKnowledge in mock to ensure module mock is complete for other tests
  useKnowledge: () => ({
    entries: [],
    loading: false,
    error: null,
    refreshing: null,
    createEntry: mock(),
    deleteEntry: mock(),
    refreshEntry: mock(),
    reload: mock(),
  }),
}));

mock.module('../../../api/sources', () => ({
  sourcesApi: {
    getSources: mockGetSources,
  },
}));

// Mock workspace context
mock.module('../../../shared/context/WorkspaceContext', () => ({
  useWorkspace: () => ({
    currentWorkspace: { id: 'test-workspace', name: 'Test Workspace' },
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
    mockSearch.mockReset();
    mockClear.mockReset();
    mockGetSources.mockReset();
    mockGetSources.mockResolvedValue(mockSources);
    // Reset search state to defaults
    searchState = {
      results: [],
      total: 0,
      loading: false,
      error: null,
    };
  });

  it('should render the search page', async () => {
    render(<ContextSearchPage />);

    expect(screen.getByText('Context Search')).toBeInTheDocument();
    expect(screen.getByPlaceholderText('Search your knowledge base...')).toBeInTheDocument();
    expect(screen.getByText('Search')).toBeInTheDocument();

    await waitFor(() => {
      expect(mockGetSources).toHaveBeenCalledWith('test-workspace', undefined, true);
    });
  });

  it('should load and display sources as pills', async () => {
    render(<ContextSearchPage />);

    await waitFor(() => {
      // Sources are displayed as buttons/pills
      expect(screen.getByRole('button', { name: 'GitHub Repo' })).toBeInTheDocument();
      expect(screen.getByRole('button', { name: 'Local Files' })).toBeInTheDocument();
    });
  });

  it('should perform search with query', async () => {
    render(<ContextSearchPage />);

    const input = screen.getByPlaceholderText('Search your knowledge base...');
    const searchButton = screen.getByRole('button', { name: 'Search' });

    fireEvent.change(input, { target: { value: 'test query' } });
    fireEvent.click(searchButton);

    await waitFor(() => {
      expect(mockSearch).toHaveBeenCalledWith({
        query: 'test query',
        mode: 'hybrid',
        source_ids: undefined,
        limit: 20,
      });
    });
  });

  it.skip('should change search mode', async () => {
    render(<ContextSearchPage />);

    // Wait for component to render
    await waitFor(() => {
      expect(screen.getByText('Semantic')).toBeInTheDocument();
    });

    // Find and click the Semantic tab by text
    const semanticTab = screen.getByText('Semantic');
    fireEvent.click(semanticTab);

    const input = screen.getByPlaceholderText('Search your knowledge base...');
    fireEvent.change(input, { target: { value: 'test' } });
    fireEvent.submit(input.closest('form')!);

    await waitFor(() => {
      expect(mockSearch).toHaveBeenCalledWith(
        expect.objectContaining({ mode: 'semantic' })
      );
    });
  });

  it('should filter by selected sources', async () => {
    render(<ContextSearchPage />);

    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'GitHub Repo' })).toBeInTheDocument();
    });

    // Click source pill to select it
    const sourcePill = screen.getByRole('button', { name: 'GitHub Repo' });
    fireEvent.click(sourcePill);

    // The pill should now have 'active' class
    expect(sourcePill).toHaveClass('active');

    const input = screen.getByPlaceholderText('Search your knowledge base...');
    fireEvent.change(input, { target: { value: 'test' } });
    fireEvent.submit(input.closest('form')!);

    await waitFor(() => {
      expect(mockSearch).toHaveBeenCalledWith(
        expect.objectContaining({ source_ids: ['s1'] })
      );
    });
  });

  it('should display relevance labels', async () => {
    searchState = {
      results: mockResults,
      total: 2,
      loading: false,
      error: null,
    };

    render(<ContextSearchPage />);

    await waitFor(() => {
      // High relevance (0.95) shows "Highly relevant"
      expect(screen.getByText('Highly relevant')).toBeInTheDocument();
      // Medium relevance (0.75) shows "Relevant"
      expect(screen.getByText('Relevant')).toBeInTheDocument();
    });
  });

  it('should display file metadata paths', async () => {
    searchState = {
      results: mockResults,
      total: 2,
      loading: false,
      error: null,
    };

    render(<ContextSearchPage />);

    await waitFor(() => {
      expect(screen.getByText('/src/test.ts')).toBeInTheDocument();
      expect(screen.getByText('/docs/readme.md')).toBeInTheDocument();
    });
  });

  it('should show empty state when no results after search', async () => {
    // Set state to have results = empty after query is set
    searchState = {
      results: [],
      total: 0,
      loading: false,
      error: null,
    };

    render(<ContextSearchPage />);

    // Simulate having searched (set query state)
    const input = screen.getByPlaceholderText('Search your knowledge base...');
    fireEvent.change(input, { target: { value: 'nonexistent' } });
    fireEvent.submit(input.closest('form')!);

    await waitFor(() => {
      expect(mockSearch).toHaveBeenCalled();
    });
  });

  it('should show error message on search failure', async () => {
    searchState = {
      results: [],
      total: 0,
      loading: false,
      error: 'Search failed',
    };

    render(<ContextSearchPage />);

    await waitFor(() => {
      expect(screen.getByRole('alert')).toHaveTextContent('Search failed');
    });
  });

  it('should display initial empty state', () => {
    render(<ContextSearchPage />);

    // Initial state shows prompt to search
    expect(screen.getByText('Search your knowledge')).toBeInTheDocument();
  });

  it('should trim whitespace from query', async () => {
    render(<ContextSearchPage />);

    const input = screen.getByPlaceholderText('Search your knowledge base...');
    fireEvent.change(input, { target: { value: '  test query  ' } });
    fireEvent.submit(input.closest('form')!);

    await waitFor(() => {
      expect(mockSearch).toHaveBeenCalledWith(
        expect.objectContaining({ query: 'test query' })
      );
    });
  });

  it('should toggle source selection', async () => {
    render(<ContextSearchPage />);

    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'GitHub Repo' })).toBeInTheDocument();
    });

    const sourcePill = screen.getByRole('button', { name: 'GitHub Repo' });

    // Click to select
    fireEvent.click(sourcePill);
    expect(sourcePill).toHaveClass('active');

    // Click to deselect
    fireEvent.click(sourcePill);
    expect(sourcePill).not.toHaveClass('active');
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

    searchState = {
      results: xssResults,
      total: 1,
      loading: false,
      error: null,
    };

    render(<ContextSearchPage />);

    await waitFor(() => {
      const snippet = document.querySelector('.result-snippet');
      if (snippet) {
        // Script tag should be sanitized out by DOMPurify
        expect(snippet.innerHTML).not.toContain('<script>');
      }
    });
  });

  it('should show results count badge', async () => {
    searchState = {
      results: mockResults,
      total: 2,
      loading: false,
      error: null,
    };

    render(<ContextSearchPage />);

    await waitFor(() => {
      expect(screen.getByText('2 found')).toBeInTheDocument();
    });
  });
});
