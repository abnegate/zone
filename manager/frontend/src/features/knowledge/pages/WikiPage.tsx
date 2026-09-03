import { Badge, Button, EmptyState, Tabs, TabsList, TabsTrigger } from '@zone/ui';
import { useState } from 'react';
import { CreateKnowledgeWizard } from '../components';
import { useKnowledge } from '../hooks';
import type { KnowledgeEntry } from '../types';
import './WikiPage.css';

type FilterType = 'all' | 'text' | 'url';

export default function WikiPage() {
  const { entries, loading, error, refreshing, createEntry, deleteEntry, refreshEntry } =
    useKnowledge();

  const [searchQuery, setSearchQuery] = useState('');
  const [filterType, setFilterType] = useState<FilterType>('all');
  const [showCreateWizard, setShowCreateWizard] = useState(false);
  const [selectedEntry, setSelectedEntry] = useState<KnowledgeEntry | null>(null);
  const [deleteError, setDeleteError] = useState<string | null>(null);
  const [refreshError, setRefreshError] = useState<string | null>(null);

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
        setSelectedEntry(null);
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
      <header className="wiki-header">
        <div className="wiki-header-copy">
          <h1>Knowledge Base</h1>
          <p>Manage documentation, links, and content for your AI models</p>
        </div>
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
        <div className="wiki-actions">
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
          <Button onClick={() => setShowCreateWizard(true)}>+ Add Knowledge</Button>
        </div>
      </header>

      {(error || deleteError || refreshError) && (
        <div className="wiki-banner wiki-banner--error" role="alert" aria-live="assertive">
          {error || deleteError || refreshError}
        </div>
      )}

      {loading ? (
        <div className="wiki-empty">
          <p>Loading knowledge...</p>
        </div>
      ) : filteredEntries.length === 0 ? (
        <EmptyState
          className="wiki-empty-state"
          icon={
            <svg
              viewBox="0 0 24 24"
              fill="none"
              stroke="currentColor"
              strokeWidth="1.5"
              width="48"
              height="48"
            >
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
            ) : undefined
          }
        />
      ) : (
        <div className="wiki-workspace">
          <div className="knowledge-grid">
            {filteredEntries.map((entry) => (
              <div
                key={entry.id}
                className="knowledge-card"
                onClick={() => setSelectedEntry(entry)}
                onKeyDown={(e) => {
                  if (e.key === 'Enter' || e.key === ' ') {
                    e.preventDefault();
                    setSelectedEntry(entry);
                  }
                }}
                role="button"
                tabIndex={0}
              >
                <div className="knowledge-card-header">
                  <h3 className="knowledge-card-title">{entry.title}</h3>
                  <Badge variant={entry.type === 'url' ? 'info' : 'secondary'}>{entry.type}</Badge>
                </div>

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

                <div className="knowledge-card-content">
                  {entry.type === 'url' && entry.fetched_content
                    ? entry.fetched_content
                    : entry.content}
                </div>

                {entry.tags.length > 0 && (
                  <div className="knowledge-card-tags">
                    {entry.tags.map((tag) => (
                      <span key={tag} className="knowledge-tag">
                        {tag}
                      </span>
                    ))}
                  </div>
                )}

                <div className="knowledge-card-footer">
                  <div className="knowledge-card-date">
                    {entry.updated_at ? <span>Updated {formatDate(entry.updated_at)}</span> : null}
                    {entry.type === 'url' && entry.last_refreshed_at && (
                      <span> • Refreshed {formatDate(entry.last_refreshed_at)}</span>
                    )}
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
              </div>
            ))}
          </div>
        </div>
      )}

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
          className="modal-overlay"
          onClick={() => setSelectedEntry(null)}
          onKeyDown={(e) => {
            if (e.key === 'Escape') {
              setSelectedEntry(null);
            }
          }}
          role="button"
          tabIndex={0}
        >
          <div
            className="modal"
            onClick={(e) => e.stopPropagation()}
            onKeyDown={(e) => e.stopPropagation()}
            role="dialog"
            aria-modal="true"
          >
            <div className="modal-header">
              <div className="flex items-center gap-3">
                <h2>{selectedEntry.title}</h2>
                <Badge variant={selectedEntry.type === 'url' ? 'info' : 'secondary'}>
                  {selectedEntry.type}
                </Badge>
              </div>
              <button
                type="button"
                className="modal-close"
                onClick={() => setSelectedEntry(null)}
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
            <div className="modal-body">
              {selectedEntry.type === 'url' && (
                <div className="form-group">
                  <span className="form-label">URL</span>
                  <a
                    href={selectedEntry.content}
                    target="_blank"
                    rel="noopener noreferrer"
                    style={{ color: 'var(--primary)' }}
                  >
                    {selectedEntry.content}
                  </a>
                </div>
              )}

              <div className="form-group">
                <span className="form-label">Content</span>
                <div style={{ whiteSpace: 'pre-wrap', color: 'var(--text-secondary)' }}>
                  {selectedEntry.type === 'url' && selectedEntry.fetched_content
                    ? selectedEntry.fetched_content
                    : selectedEntry.content}
                </div>
              </div>

              {selectedEntry.tags.length > 0 && (
                <div className="form-group">
                  <span className="form-label">Tags</span>
                  <div className="knowledge-card-tags">
                    {selectedEntry.tags.map((tag) => (
                      <span key={tag} className="knowledge-tag">
                        {tag}
                      </span>
                    ))}
                  </div>
                </div>
              )}

              <div className="form-group">
                <span className="form-label">Details</span>
                <div style={{ color: 'var(--text-secondary)', fontSize: '0.9rem' }}>
                  <p>Created: {formatDate(selectedEntry.created_at)}</p>
                  <p>Updated: {formatDate(selectedEntry.updated_at)}</p>
                  {selectedEntry.type === 'url' && selectedEntry.last_refreshed_at && (
                    <p>Last Refreshed: {formatDate(selectedEntry.last_refreshed_at)}</p>
                  )}
                </div>
              </div>
            </div>
            <div className="modal-footer">
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
              <Button
                variant="destructive"
                onClick={() => {
                  handleDeleteKnowledge(selectedEntry.id);
                  setSelectedEntry(null);
                }}
              >
                Delete
              </Button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
