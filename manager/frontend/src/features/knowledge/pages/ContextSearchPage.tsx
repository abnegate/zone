import { Badge, Button, EmptyState, Tabs, TabsList, TabsTrigger } from '@zone/ui';
import DOMPurify from 'dompurify';
import { useEffect, useState } from 'react';
import { sourcesApi } from '../../../api/sources';
import PageBar from '../../../shared/components/PageBar/PageBar';
import { useWorkspace } from '../../../shared/context/WorkspaceContext';
import type { Source } from '../../sources/types';
import { useContextSearch } from '../hooks';
import type { SearchMode } from '../types';
import './ContextSearchPage.css';

const SearchIcon = () => (
  <svg
    width="16"
    height="16"
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    strokeWidth="2"
    strokeLinecap="round"
    strokeLinejoin="round"
    aria-hidden="true"
  >
    <circle cx="11" cy="11" r="8" />
    <path d="m21 21-4.35-4.35" />
  </svg>
);

const FileIcon = () => (
  <svg
    width="12"
    height="12"
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    strokeWidth="2"
    strokeLinecap="round"
    strokeLinejoin="round"
    aria-hidden="true"
  >
    <path d="M15 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V7Z" />
    <path d="M14 2v4a2 2 0 0 0 2 2h4" />
  </svg>
);

const FolderIcon = () => (
  <svg
    width="14"
    height="14"
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    strokeWidth="2"
    strokeLinecap="round"
    strokeLinejoin="round"
    aria-hidden="true"
  >
    <path d="M20 20a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-7.9a2 2 0 0 1-1.69-.9L9.6 3.9A2 2 0 0 0 7.93 3H4a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2Z" />
  </svg>
);

const SparklesIcon = () => (
  <svg
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    strokeWidth="1.5"
    strokeLinecap="round"
    strokeLinejoin="round"
    aria-hidden="true"
  >
    <path d="m12 3-1.912 5.813a2 2 0 0 1-1.275 1.275L3 12l5.813 1.912a2 2 0 0 1 1.275 1.275L12 21l1.912-5.813a2 2 0 0 1 1.275-1.275L21 12l-5.813-1.912a2 2 0 0 1-1.275-1.275L12 3Z" />
    <path d="M5 3v4" />
    <path d="M19 17v4" />
    <path d="M3 5h4" />
    <path d="M17 19h4" />
  </svg>
);

const SearchEmptyIcon = () => (
  <svg
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    strokeWidth="1.5"
    strokeLinecap="round"
    strokeLinejoin="round"
    aria-hidden="true"
  >
    <circle cx="11" cy="11" r="8" />
    <path d="m21 21-4.35-4.35" />
    <path d="M8 8l6 6" />
    <path d="M14 8l-6 6" />
  </svg>
);

type RelevanceLevel = 'high' | 'medium' | 'low';

const relevanceLevel = (score: number): RelevanceLevel => {
  if (score >= 0.8) return 'high';
  if (score >= 0.5) return 'medium';
  return 'low';
};

const RELEVANCE_LABELS: Record<RelevanceLevel, string> = {
  high: 'Highly relevant',
  medium: 'Relevant',
  low: 'Partial match',
};

const RELEVANCE_VARIANTS: Record<RelevanceLevel, 'success' | 'warning' | 'neutral'> = {
  high: 'success',
  medium: 'warning',
  low: 'neutral',
};

const escapeRegex = (str: string) => str.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');

export default function ContextSearchPage() {
  const [query, setQuery] = useState('');
  const [mode, setMode] = useState<SearchMode>('hybrid');
  const [sources, setSources] = useState<Source[]>([]);
  const [selectedSources, setSelectedSources] = useState<string[]>([]);
  const [sourcesLoading, setSourcesLoading] = useState(true);
  const { currentWorkspace } = useWorkspace();
  const workspaceId = currentWorkspace?.id;

  const { results, total, loading, error, search } = useContextSearch();

  useEffect(() => {
    let mounted = true;

    const loadSources = async () => {
      if (!workspaceId) {
        setSources([]);
        setSourcesLoading(false);
        return;
      }
      try {
        setSourcesLoading(true);
        const data = await sourcesApi.getSources(workspaceId, undefined, true);
        if (mounted) {
          setSources(data);
        }
      } catch (err) {
        if (mounted) {
          console.error('Failed to load sources:', err);
        }
      } finally {
        if (mounted) {
          setSourcesLoading(false);
        }
      }
    };

    loadSources();

    return () => {
      mounted = false;
    };
  }, [workspaceId]);

  const handleSearch = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!query.trim()) return;

    await search({
      query: query.trim(),
      mode,
      source_ids: selectedSources.length > 0 ? selectedSources : undefined,
      limit: 20,
    });
  };

  const toggleSource = (sourceId: string) => {
    setSelectedSources((prev) =>
      prev.includes(sourceId) ? prev.filter((id) => id !== sourceId) : [...prev, sourceId]
    );
  };

  const highlightText = (text: string) => {
    const queryTerms = query.toLowerCase().split(/\s+/);
    let highlighted = text
      .replaceAll('&', '&amp;')
      .replaceAll('<', '&lt;')
      .replaceAll('>', '&gt;')
      .replaceAll('"', '&quot;')
      .replaceAll("'", '&#039;');

    queryTerms.forEach((term) => {
      if (term.length > 2) {
        const regex = new RegExp(`(${escapeRegex(term)})`, 'gi');
        highlighted = highlighted.replace(regex, '<mark>$1</mark>');
      }
    });

    return DOMPurify.sanitize(highlighted);
  };

  const resultScoreLabel = (result: {
    relevance_score: number;
    metadata: Record<string, unknown>;
  }) => {
    const semantic = result.metadata.semantic_score;
    if (typeof semantic === 'number') {
      return `${Math.round(semantic * 100)}% semantic`;
    }
    if (typeof result.metadata.keyword_score === 'number') {
      return 'Keyword match';
    }
    return RELEVANCE_LABELS[relevanceLevel(result.relevance_score)];
  };

  return (
    <div className="page page--workspace context-search-page">
      <PageBar
        title="Context Search"
        subtitle="Search across all your connected knowledge sources"
      />

      <div className="page-body context-search-body">
        <div className="page-container context-search-content">
          <div className="search-toolbar">
            <form onSubmit={handleSearch} className="search-form">
              <div className="search-input-wrapper">
                <span className="search-icon-wrapper">
                  <SearchIcon />
                </span>
                <input
                  type="text"
                  value={query}
                  onChange={(e) => setQuery(e.target.value)}
                  placeholder="Search your knowledge base..."
                  className="search-input"
                  disabled={loading}
                />
              </div>
              <Button type="submit" disabled={loading || !query.trim()}>
                {loading ? <span className="ui-btn-spinner" /> : 'Search'}
              </Button>
            </form>

            <div className="filter-group">
              <span className="filter-label">Mode</span>
              <Tabs value={mode} onValueChange={(v) => setMode(v as SearchMode)}>
                <TabsList>
                  <TabsTrigger value="hybrid">Hybrid</TabsTrigger>
                  <TabsTrigger value="semantic">Semantic</TabsTrigger>
                  <TabsTrigger value="keyword">Keyword</TabsTrigger>
                </TabsList>
              </Tabs>
            </div>

            {sources.length > 0 && (
              <div className="filter-group">
                <span className="filter-label">Sources</span>
                <div className="source-pills">
                  {sourcesLoading ? (
                    <span className="filter-loading">Loading...</span>
                  ) : (
                    sources.map((source) => (
                      <button
                        key={source.id}
                        type="button"
                        className={`source-pill ${selectedSources.includes(source.id) ? 'active' : ''}`}
                        onClick={() => toggleSource(source.id)}
                        disabled={loading}
                      >
                        {source.name}
                      </button>
                    ))
                  )}
                </div>
              </div>
            )}
          </div>

          {error && (
            <div className="error-banner" role="alert">
              <span>{error}</span>
              <Button variant="ghost" size="sm" onClick={() => search({ query, mode, limit: 20 })}>
                Retry
              </Button>
            </div>
          )}

          {results.length > 0 && (
            <div className="results-section">
              <div className="results-header">
                <h2 className="results-title">Results</h2>
                <Badge variant="neutral">{total} found</Badge>
              </div>

              <div className="results-grid">
                {results.map((result) => {
                  const level = relevanceLevel(result.relevance_score);
                  const path = typeof result.metadata.path === 'string' ? result.metadata.path : '';
                  return (
                    <article key={result.id} className="card card--list result-card">
                      <div className="result-card-header">
                        <span className="result-source">
                          <FolderIcon />
                          <span className="result-source-name">{result.source_name}</span>
                        </span>
                        <Badge variant={RELEVANCE_VARIANTS[level]}>
                          {resultScoreLabel(result)}
                        </Badge>
                      </div>

                      <div
                        className="result-snippet"
                        // biome-ignore lint/security/noDangerouslySetInnerHtml: Sanitized with DOMPurify
                        dangerouslySetInnerHTML={{ __html: highlightText(result.snippet) }}
                      />

                      {path && (
                        <div className="result-meta">
                          <FileIcon />
                          <span className="result-path">{path}</span>
                        </div>
                      )}
                    </article>
                  );
                })}
              </div>
            </div>
          )}

          {!loading && results.length === 0 && query && !error && (
            <EmptyState
              icon={<SearchEmptyIcon />}
              title="No results found"
              description="Try adjusting your search terms or broadening your filters"
              action={
                <Button variant="outline" onClick={() => setQuery('')}>
                  Clear search
                </Button>
              }
            />
          )}

          {!loading && !query && (
            <EmptyState
              icon={<SparklesIcon />}
              title="Search your knowledge"
              description="Enter a query to search across all your connected sources using semantic, keyword, or hybrid search"
            />
          )}
        </div>
      </div>
    </div>
  );
}
