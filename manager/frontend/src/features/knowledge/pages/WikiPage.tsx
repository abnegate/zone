import { Badge, Button, EmptyState, Tabs, TabsList, TabsTrigger } from '@zone/ui';
import { useCallback, useEffect, useRef, useState } from 'react';
import { useSearchParams } from 'react-router-dom';
import PageBar from '../../../shared/components/PageBar/PageBar';
import PlusIcon from '../../../shared/components/PlusIcon/PlusIcon';
import { CreateKnowledgeWizard } from '../components';
import { useKnowledge } from '../hooks';
import type { KnowledgeEntry } from '../types';
import './WikiPage.css';

type FilterType = 'all' | 'text' | 'url';

/// What a card says about itself when the list carries no date: the size of
/// the entry, or failing that its type, so the meta row is never blank.
function cardSize(entry: KnowledgeEntry): string {
  return entry.token_count === null ? entry.type : `${entry.token_count.toLocaleString()} tokens`;
}

export default function WikiPage() {
  const { entries, loading, error, refreshing, createEntry, deleteEntry, refreshEntry, readEntry } =
    useKnowledge();

  const [searchParams, setSearchParams] = useSearchParams();
  const [searchQuery, setSearchQuery] = useState('');
  const [filterType, setFilterType] = useState<FilterType>('all');
  const [showCreateWizard, setShowCreateWizard] = useState(false);
  const [selectedEntry, setSelectedEntry] = useState<KnowledgeEntry | null>(null);
  const linkedEntryId = searchParams.get('id');
  const [deleteError, setDeleteError] = useState<string | null>(null);
  const [refreshError, setRefreshError] = useState<string | null>(null);

  const listed = useRef<KnowledgeEntry[]>(entries);
  useEffect(() => {
    listed.current = entries;
  }, [entries]);

  /// A citation names the entry by id, and the list is one page of a workspace
  /// that carries no content, so the entry is read on its own rather than
  /// looked up in what happens to be loaded. A read that fails falls back to
  /// the loaded page, which is all this ever had.
  useEffect(() => {
    if (!linkedEntryId) return;
    let live = true;
    readEntry(linkedEntryId)
      .then((entry) => {
        if (live) setSelectedEntry(entry);
      })
      .catch(() => {
        const known = listed.current.find((entry) => entry.id === linkedEntryId);
        if (live && known) setSelectedEntry(known);
      });
    return () => {
      live = false;
    };
  }, [linkedEntryId, readEntry]);

  /// The card carries what the list gave it; the entry's own content arrives
  /// with the read, so the reader sees the passage rather than an empty panel.
  const openEntry = useCallback(
    (entry: KnowledgeEntry) => {
      setSelectedEntry(entry);
      readEntry(entry.id)
        .then((full) => {
          setSelectedEntry((shown) => (shown?.id === full.id ? full : shown));
        })
        .catch(() => undefined);
    },
    [readEntry]
  );

  const closeSelectedEntry = () => {
    setSelectedEntry(null);
    if (searchParams.get('id')) {
      const next = new URLSearchParams(searchParams);
      next.delete('id');
      setSearchParams(next, { replace: true });
    }
  };

  const handleEntryCreated = (_entry: KnowledgeEntry) => {
    // Entry is already added to the list by the hook
  };

  const handleDeleteKnowledge = async (id: string) => {
    if (!window.confirm('Are you sure you want to delete this knowledge entry?')) {
      return;
    }

    try {
      setDeleteError(null);
      await deleteEntry(id);
      if (selectedEntry?.id === id) {
        closeSelectedEntry();
      }
    } catch (err) {
      setDeleteError(err instanceof Error ? err.message : 'Failed to delete knowledge');
    }
  };

  const handleRefreshKnowledge = async (id: string) => {
    try {
      setRefreshError(null);
      const refreshedEntry = await refreshEntry(id);
      if (selectedEntry?.id === id) {
        setSelectedEntry(refreshedEntry);
      }
    } catch (err) {
      setRefreshError(err instanceof Error ? err.message : 'Failed to refresh knowledge');
    }
  };

  const filteredEntries = entries.filter((entry) => {
    const matchesFilter = filterType === 'all' || entry.type === filterType;
    const matchesSearch =
      searchQuery === '' ||
      entry.title.toLowerCase().includes(searchQuery.toLowerCase()) ||
      entry.content.toLowerCase().includes(searchQuery.toLowerCase()) ||
      (entry.fetched_content?.toLowerCase().includes(searchQuery.toLowerCase()) ?? false) ||
      entry.tags.some((tag) => tag.toLowerCase().includes(searchQuery.toLowerCase()));
    return matchesFilter && matchesSearch;
  });

  const formatDate = (date: string) => {
    if (!date) return '—';
    const parsed = new Date(date);
    if (Number.isNaN(parsed.getTime())) return '—';
    return parsed.toLocaleDateString(undefined, {
      year: 'numeric',
      month: 'short',
      day: 'numeric',
    });
  };

  return (
    <div className="page page--workspace wiki-page">
      <PageBar
        title="Knowledge Base"
        subtitle="Manage documentation, links, and content for your AI models"
      >
        <Tabs
          value={filterType}
          onValueChange={(v) => setFilterType(v as FilterType)}
          className="wiki-tabs"
        >
          <TabsList>
            <TabsTrigger value="all">All</TabsTrigger>
            <TabsTrigger value="text">Text</TabsTrigger>
            <TabsTrigger value="url">URL</TabsTrigger>
          </TabsList>
        </Tabs>
        <div className="wiki-search">
          <svg
            className="wiki-search-icon"
            fill="none"
            stroke="currentColor"
            viewBox="0 0 24 24"
            aria-hidden="true"
          >
            <path
              strokeLinecap="round"
              strokeLinejoin="round"
              strokeWidth={2}
              d="M21 21l-6-6m2-5a7 7 0 11-14 0 7 7 0 0114 0z"
            />
          </svg>
          <input
            type="search"
            placeholder="Search knowledge..."
            value={searchQuery}
            onChange={(e) => setSearchQuery(e.target.value)}
            aria-label="Search knowledge"
          />
        </div>
        <Button onClick={() => setShowCreateWizard(true)}>
          <PlusIcon />
          Add knowledge
        </Button>
      </PageBar>

      <div className="page-body wiki-body">
        {(error || deleteError || refreshError) && (
          <div className="wiki-banner wiki-banner--error" role="alert" aria-live="assertive">
            {error || deleteError || refreshError}
          </div>
        )}

        {loading ? (
          <div className="loading-state">
            <span className="loading-spinner" aria-hidden="true" />
            <span className="loading-text">Loading knowledge...</span>
          </div>
        ) : filteredEntries.length === 0 ? (
          <EmptyState
            className="wiki-empty-state"
            icon={
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5">
                <path d="M12 6.253v13m0-13C10.832 5.477 9.246 5 7.5 5S4.168 5.477 3 6.253v13C4.168 18.477 5.754 18 7.5 18s3.332.477 4.5 1.253m0-13C13.168 5.477 14.754 5 16.5 5c1.747 0 3.332.477 4.5 1.253v13C19.832 18.477 18.247 18 16.5 18c-1.746 0-3.332.477-4.5 1.253" />
              </svg>
            }
            title="No knowledge entries found"
            description={
              searchQuery || filterType !== 'all'
                ? 'Try adjusting your filters or search query'
                : 'Add your first knowledge entry to build your knowledge base'
            }
            action={
              !searchQuery && filterType === 'all' ? (
                <Button onClick={() => setShowCreateWizard(true)}>Add Entry</Button>
              ) : (
                <Button
                  variant="secondary"
                  onClick={() => {
                    setFilterType('all');
                    setSearchQuery('');
                  }}
                >
                  Show all entries
                </Button>
              )
            }
          />
        ) : (
          <div className="knowledge-grid">
            {filteredEntries.map((entry) => {
              const excerpt = entry.excerpt;
              const updated = entry.updated_at || entry.created_at;
              return (
                <div
                  key={entry.id}
                  className="knowledge-card"
                  onClick={() => openEntry(entry)}
                  onKeyDown={(e) => {
                    if (e.key === 'Enter' || e.key === ' ') {
                      e.preventDefault();
                      openEntry(entry);
                    }
                  }}
                  role="button"
                  tabIndex={0}
                >
                  <div className="knowledge-card-header">
                    <h3 className="knowledge-card-title">{entry.title}</h3>
                    {entry.indexed === false && (
                      <Badge variant="warning" title="Semantic search cannot find this entry yet">
                        Not indexed
                      </Badge>
                    )}
                    <Badge variant={entry.type === 'url' ? 'info' : 'neutral'}>{entry.type}</Badge>
                  </div>

                  {excerpt ? (
                    <div className="knowledge-card-content">{excerpt}</div>
                  ) : entry.tags.length > 0 ? (
                    <div className="knowledge-card-tags">
                      {entry.tags.map((tag) => (
                        <span key={tag} className="knowledge-tag">
                          {tag}
                        </span>
                      ))}
                    </div>
                  ) : entry.category ? (
                    <div className="knowledge-card-content knowledge-card-content--empty">
                      {entry.category}
                    </div>
                  ) : null}

                  <div className="knowledge-card-footer">
                    <span className="knowledge-card-date">
                      {updated ? `Updated ${formatDate(updated)}` : cardSize(entry)}
                      {entry.type === 'url' && entry.last_refreshed_at
                        ? ` · Refreshed ${formatDate(entry.last_refreshed_at)}`
                        : ''}
                    </span>
                    {entry.type === 'url' && (
                      <a
                        href={entry.content}
                        className="knowledge-card-url"
                        onClick={(e) => e.stopPropagation()}
                        target="_blank"
                        rel="noopener noreferrer"
                      >
                        {entry.content}
                      </a>
                    )}
                    {excerpt && entry.tags.length > 0 ? (
                      <span className="knowledge-card-tagline">
                        {entry.tags.map((tag) => (
                          <span key={tag} className="knowledge-card-tagline-item">
                            {tag}
                          </span>
                        ))}
                      </span>
                    ) : null}
                  </div>

                  <div className="knowledge-card-actions">
                    {entry.type === 'url' && (
                      <button
                        type="button"
                        className={`knowledge-action-btn refresh ${refreshing === entry.id ? 'refreshing' : ''}`}
                        onClick={(e) => {
                          e.stopPropagation();
                          handleRefreshKnowledge(entry.id);
                        }}
                        disabled={refreshing === entry.id}
                        title="Refresh URL content"
                        aria-label="Refresh URL content"
                      >
                        <svg
                          fill="none"
                          stroke="currentColor"
                          viewBox="0 0 24 24"
                          aria-hidden="true"
                        >
                          <path
                            strokeLinecap="round"
                            strokeLinejoin="round"
                            strokeWidth={2}
                            d="M4 4v5h.582m15.356 2A8.001 8.001 0 004.582 9m0 0H9m11 11v-5h-.581m0 0a8.003 8.003 0 01-15.357-2m15.357 2H15"
                          />
                        </svg>
                      </button>
                    )}
                    <button
                      type="button"
                      className="knowledge-action-btn delete"
                      onClick={(e) => {
                        e.stopPropagation();
                        handleDeleteKnowledge(entry.id);
                      }}
                      title="Delete entry"
                      aria-label="Delete entry"
                    >
                      <svg fill="none" stroke="currentColor" viewBox="0 0 24 24" aria-hidden="true">
                        <path
                          strokeLinecap="round"
                          strokeLinejoin="round"
                          strokeWidth={2}
                          d="M19 7l-.867 12.142A2 2 0 0116.138 21H7.862a2 2 0 01-1.995-1.858L5 7m5 4v6m4-6v6m1-10V4a1 1 0 00-1-1h-4a1 1 0 00-1 1v3M4 7h16"
                        />
                      </svg>
                    </button>
                  </div>
                </div>
              );
            })}
          </div>
        )}
      </div>

      {/* Create Knowledge Wizard */}
      <CreateKnowledgeWizard
        isOpen={showCreateWizard}
        onClose={() => setShowCreateWizard(false)}
        onCreated={handleEntryCreated}
        createEntry={createEntry}
      />

      {/* View Entry Modal */}
      {selectedEntry && !showCreateWizard && (
        <div
          className="wiki-dialog-overlay"
          onClick={closeSelectedEntry}
          onKeyDown={(e) => {
            if (e.key === 'Escape') {
              closeSelectedEntry();
            }
          }}
          role="button"
          tabIndex={0}
        >
          <div
            className="wiki-dialog"
            onClick={(e) => e.stopPropagation()}
            onKeyDown={(e) => e.stopPropagation()}
            role="dialog"
            aria-modal="true"
            aria-labelledby="knowledge-entry-title"
          >
            <div className="wiki-dialog-header">
              <h2 id="knowledge-entry-title">{selectedEntry.title}</h2>
              <Badge variant={selectedEntry.type === 'url' ? 'info' : 'neutral'}>
                {selectedEntry.type}
              </Badge>
              <button
                type="button"
                className="wiki-dialog-close"
                onClick={closeSelectedEntry}
                aria-label="Close modal"
              >
                <svg fill="none" stroke="currentColor" viewBox="0 0 24 24" aria-hidden="true">
                  <path
                    strokeLinecap="round"
                    strokeLinejoin="round"
                    strokeWidth={2}
                    d="M6 18L18 6M6 6l12 12"
                  />
                </svg>
              </button>
            </div>
            <div className="wiki-dialog-body">
              {selectedEntry.type === 'url' && (
                <a
                  href={selectedEntry.content}
                  className="wiki-dialog-url"
                  target="_blank"
                  rel="noopener noreferrer"
                >
                  {selectedEntry.content}
                </a>
              )}

              <div className="wiki-dialog-content">
                {selectedEntry.type === 'url' && selectedEntry.fetched_content
                  ? selectedEntry.fetched_content
                  : selectedEntry.content}
              </div>

              {selectedEntry.tags.length > 0 && (
                <div className="knowledge-card-tags">
                  {selectedEntry.tags.map((tag) => (
                    <span key={tag} className="knowledge-tag">
                      {tag}
                    </span>
                  ))}
                </div>
              )}

              <dl className="wiki-dialog-details">
                <dt>Created</dt>
                <dd>{formatDate(selectedEntry.created_at)}</dd>
                <dt>Updated</dt>
                <dd>{formatDate(selectedEntry.updated_at)}</dd>
                {selectedEntry.type === 'url' && selectedEntry.last_refreshed_at && (
                  <>
                    <dt>Last refreshed</dt>
                    <dd>{formatDate(selectedEntry.last_refreshed_at)}</dd>
                  </>
                )}
              </dl>
            </div>
            <div className="wiki-dialog-footer">
              <Button
                variant="destructive"
                onClick={() => {
                  handleDeleteKnowledge(selectedEntry.id);
                  closeSelectedEntry();
                }}
              >
                Delete
              </Button>
              <span className="wiki-dialog-footer-spacer" />
              {selectedEntry.type === 'url' && (
                <Button
                  variant="secondary"
                  onClick={() => handleRefreshKnowledge(selectedEntry.id)}
                  disabled={refreshing === selectedEntry.id}
                  loading={refreshing === selectedEntry.id}
                >
                  {refreshing === selectedEntry.id ? 'Refreshing...' : 'Refresh Content'}
                </Button>
              )}
              <Button variant="secondary" onClick={closeSelectedEntry}>
                Close
              </Button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
