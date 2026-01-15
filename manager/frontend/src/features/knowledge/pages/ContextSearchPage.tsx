import { useEffect, useState } from 'react';
import DOMPurify from 'dompurify';
import { sourcesApi } from '../../../api/sources';
import { useWorkspace } from '../../../shared/context/WorkspaceContext';
import type { Source } from '../../sources/types';
import type { SearchMode } from '../types';
import { useContextSearch } from '../hooks';
import './ContextSearchPage.css';

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
        const data = await sourcesApi.getSources(workspaceId, undefined, true); // Only active sources
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

  const escapeRegex = (str: string) => str.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');

  const highlightText = (text: string) => {
    // Simple highlighting - in production, the backend should provide pre-highlighted snippets
    const queryTerms = query.toLowerCase().split(/\s+/);
    let highlighted = text;

    queryTerms.forEach((term) => {
      if (term.length > 2) {
        // Escape special regex characters to prevent regex injection
        const regex = new RegExp(`(${escapeRegex(term)})`, 'gi');
        highlighted = highlighted.replace(regex, '<mark>$1</mark>');
      }
    });

    // Sanitize the output to prevent XSS attacks
    return DOMPurify.sanitize(highlighted);
  };

  const getRelevanceColor = (score: number) => {
    if (score >= 0.8) return 'high';
    if (score >= 0.5) return 'medium';
    return 'low';
  };

  return (
    <div className="context-search-page">
      <header className="page-header">
        <h1>Context Search</h1>
        <p>Search across all your connected sources</p>
      </header>

      <div className="search-container">
        <form onSubmit={handleSearch} className="search-form">
          <div className="search-input-row">
            <input
              type="text"
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              placeholder="Search your context..."
              className="search-input"
              disabled={loading}
            />
            <button type="submit" disabled={loading || !query.trim()} className="search-button">
              {loading ? (
                <span className="spinner" />
              ) : (
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
                  <circle cx="11" cy="11" r="8" />
                  <path d="m21 21-4.35-4.35" />
                </svg>
              )}
              Search
            </button>
          </div>

          <div className="search-options">
            <div className="search-mode" role="group" aria-label="Search Mode">
              <span className="option-label">Search Mode:</span>
              <div className="mode-buttons">
                <button
                  type="button"
                  className={`mode-btn ${mode === 'hybrid' ? 'active' : ''}`}
                  onClick={() => setMode('hybrid')}
                  disabled={loading}
                >
                  Hybrid
                </button>
                <button
                  type="button"
                  className={`mode-btn ${mode === 'semantic' ? 'active' : ''}`}
                  onClick={() => setMode('semantic')}
                  disabled={loading}
                >
                  Semantic
                </button>
                <button
                  type="button"
                  className={`mode-btn ${mode === 'keyword' ? 'active' : ''}`}
                  onClick={() => setMode('keyword')}
                  disabled={loading}
                >
                  Keyword
                </button>
              </div>
            </div>

            <div className="source-filter" role="group" aria-label="Filter by Sources">
              <span className="option-label">Filter by Sources:</span>
              {sourcesLoading ? (
                <span className="sources-loading">Loading sources...</span>
              ) : sources.length === 0 ? (
                <span className="no-sources">No sources available</span>
              ) : (
                <div className="source-checkboxes">
                  {sources.map((source) => (
                    <label key={source.id} className="source-checkbox">
                      <input
                        type="checkbox"
                        checked={selectedSources.includes(source.id)}
                        onChange={() => toggleSource(source.id)}
                        disabled={loading}
                      />
                      <span>{source.name}</span>
                    </label>
                  ))}
                </div>
              )}
            </div>
          </div>
        </form>

        {error && (
          <div className="error-message" role="alert">
            {error}
          </div>
        )}

        {results.length > 0 && (
          <div className="search-results">
            <div className="results-header">
              <h2>Results</h2>
              <span className="results-count">
                {total} {total === 1 ? 'result' : 'results'} found
              </span>
            </div>

            <div className="results-list">
              {results.map((result) => (
                <div key={result.id} className="result-item">
                  <div className="result-header">
                    <div className="result-source">
                      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
                        <path d="M5 19a2 2 0 01-2-2V7a2 2 0 012-2h4l2 2h4a2 2 0 012 2v1M5 19h14a2 2 0 002-2v-5a2 2 0 00-2-2H9a2 2 0 00-2 2v5a2 2 0 01-2 2z" />
                      </svg>
                      <span>{result.source_name}</span>
                    </div>
                    <div
                      className={`result-relevance ${getRelevanceColor(result.relevance_score)}`}
                    >
                      <div
                        className="relevance-bar"
                        style={{ width: `${result.relevance_score * 100}%` }}
                      />
                      <span className="relevance-label">
                        {Math.round(result.relevance_score * 100)}% relevant
                      </span>
                    </div>
                  </div>

                  <div
                    className="result-snippet"
                    // biome-ignore lint/security/noDangerouslySetInnerHtml: Required for search term highlighting; snippets sanitized by backend
                    dangerouslySetInnerHTML={{ __html: highlightText(result.snippet) }}
                  />

                  {Object.keys(result.metadata).length > 0 && (
                    <div className="result-metadata">
                      {result.metadata.type === 'file' &&
                      result.metadata.path &&
                      typeof result.metadata.path === 'string' ? (
                        <span className="metadata-item">
                          <svg
                            viewBox="0 0 24 24"
                            fill="none"
                            stroke="currentColor"
                            strokeWidth="2"
                          >
                            <path d="M9 12h6m-6 4h6m2 5H7a2 2 0 01-2-2V5a2 2 0 012-2h5.586a1 1 0 01.707.293l5.414 5.414a1 1 0 01.293.707V19a2 2 0 01-2 2z" />
                          </svg>
                          {result.metadata.path}
                        </span>
                      ) : null}
                    </div>
                  )}
                </div>
              ))}
            </div>
          </div>
        )}

        {!loading && results.length === 0 && query && !error && (
          <div className="empty-state">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
              <circle cx="11" cy="11" r="8" />
              <path d="m21 21-4.35-4.35" />
            </svg>
            <h3>No results found</h3>
            <p>Try adjusting your search query or filters</p>
          </div>
        )}

        {!loading && !query && (
          <div className="empty-state">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
              <circle cx="11" cy="11" r="8" />
              <path d="m21 21-4.35-4.35" />
            </svg>
            <h3>Start searching</h3>
            <p>Enter a query to search across your connected sources</p>
          </div>
        )}
      </div>
    </div>
  );
}
