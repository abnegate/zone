import { useState } from 'react';
import type { CreateKnowledgeRequest, KnowledgeEntry, KnowledgeType } from '../types';
import { getErrors } from '../../../validation';
import { CreateKnowledgeRequestSchema } from '../schemas';
import { useKnowledge } from '../hooks';
import './WikiPage.css';

type FilterType = 'all' | 'text' | 'url';

export default function WikiPage() {
  const { entries, loading, error, refreshing, createEntry, deleteEntry, refreshEntry } =
    useKnowledge();

  const [searchQuery, setSearchQuery] = useState('');
  const [filterType, setFilterType] = useState<FilterType>('all');
  const [showModal, setShowModal] = useState(false);
  const [selectedEntry, setSelectedEntry] = useState<KnowledgeEntry | null>(null);

  // Form state
  const [formData, setFormData] = useState<CreateKnowledgeRequest>({
    title: '',
    type: 'text',
    content: '',
    tags: [],
  });
  const [formErrors, setFormErrors] = useState<Record<string, string>>({});
  const [submitting, setSubmitting] = useState(false);
  const [tagInput, setTagInput] = useState('');

  const handleCreateKnowledge = async (e: React.FormEvent) => {
    e.preventDefault();

    const errors = getErrors(CreateKnowledgeRequestSchema, formData);
    if (Object.keys(errors).length > 0) {
      setFormErrors(errors);
      return;
    }

    try {
      setSubmitting(true);
      setFormErrors({});
      await createEntry(formData);
      setShowModal(false);
      resetForm();
    } catch (err) {
      setFormErrors({ _root: err instanceof Error ? err.message : 'Failed to create knowledge' });
    } finally {
      setSubmitting(false);
    }
  };

  const [deleteError, setDeleteError] = useState<string | null>(null);
  const [refreshError, setRefreshError] = useState<string | null>(null);

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

  const resetForm = () => {
    setFormData({
      title: '',
      type: 'text',
      content: '',
      tags: [],
    });
    setTagInput('');
    setFormErrors({});
  };

  const handleAddTag = (e: React.KeyboardEvent<HTMLInputElement>) => {
    if (e.key === 'Enter' && tagInput.trim()) {
      e.preventDefault();
      if (!formData.tags?.includes(tagInput.trim())) {
        setFormData({
          ...formData,
          tags: [...(formData.tags || []), tagInput.trim()],
        });
      }
      setTagInput('');
    }
  };

  const handleRemoveTag = (tag: string) => {
    setFormData({
      ...formData,
      tags: formData.tags?.filter((t) => t !== tag) || [],
    });
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
    return new Date(date).toLocaleDateString(undefined, {
      year: 'numeric',
      month: 'short',
      day: 'numeric',
    });
  };

  return (
    <div className="page wiki-page">
      <div className="wiki-header">
        <div>
          <h1>Knowledge Base</h1>
          <p className="wiki-subtitle">
            Manage documentation, links, and content for your AI models
          </p>
        </div>
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
              type="text"
              placeholder="Search knowledge..."
              value={searchQuery}
              onChange={(e) => setSearchQuery(e.target.value)}
              aria-label="Search knowledge"
            />
          </div>
          <button
            type="button"
            className="add-knowledge-btn"
            onClick={() => {
              resetForm();
              setShowModal(true);
            }}
          >
            <svg fill="none" stroke="currentColor" viewBox="0 0 24 24" aria-hidden="true">
              <path
                strokeLinecap="round"
                strokeLinejoin="round"
                strokeWidth={2}
                d="M12 4v16m8-8H4"
              />
            </svg>
            Add Knowledge
          </button>
        </div>
      </div>

      <div className="wiki-filters">
        <button
          type="button"
          className={`filter-btn ${filterType === 'all' ? 'active' : ''}`}
          onClick={() => setFilterType('all')}
        >
          All
        </button>
        <button
          type="button"
          className={`filter-btn ${filterType === 'text' ? 'active' : ''}`}
          onClick={() => setFilterType('text')}
        >
          Text
        </button>
        <button
          type="button"
          className={`filter-btn ${filterType === 'url' ? 'active' : ''}`}
          onClick={() => setFilterType('url')}
        >
          URL
        </button>
      </div>

      {error && (
        <div className="alert alert-error" role="alert">
          {error}
        </div>
      )}

      {deleteError && (
        <div className="alert alert-error" role="alert">
          {deleteError}
        </div>
      )}

      {refreshError && (
        <div className="alert alert-error" role="alert">
          {refreshError}
        </div>
      )}

      {loading ? (
        <div className="wiki-empty">
          <p>Loading knowledge...</p>
        </div>
      ) : filteredEntries.length === 0 ? (
        <div className="wiki-empty">
          <svg
            className="wiki-empty-icon"
            fill="none"
            stroke="currentColor"
            viewBox="0 0 24 24"
            aria-hidden="true"
          >
            <path
              strokeLinecap="round"
              strokeLinejoin="round"
              strokeWidth={1.5}
              d="M12 6.253v13m0-13C10.832 5.477 9.246 5 7.5 5S4.168 5.477 3 6.253v13C4.168 18.477 5.754 18 7.5 18s3.332.477 4.5 1.253m0-13C13.168 5.477 14.754 5 16.5 5c1.747 0 3.332.477 4.5 1.253v13C19.832 18.477 18.247 18 16.5 18c-1.746 0-3.332.477-4.5 1.253"
            />
          </svg>
          <h2>No knowledge entries found</h2>
          <p>
            {searchQuery || filterType !== 'all'
              ? 'Try adjusting your filters or search query'
              : 'Get started by adding your first knowledge entry'}
          </p>
          {!searchQuery && filterType === 'all' && (
            <button
              type="button"
              className="add-knowledge-btn"
              onClick={() => {
                resetForm();
                setShowModal(true);
              }}
            >
              <svg fill="none" stroke="currentColor" viewBox="0 0 24 24" aria-hidden="true">
                <path
                  strokeLinecap="round"
                  strokeLinejoin="round"
                  strokeWidth={2}
                  d="M12 4v16m8-8H4"
                />
              </svg>
              Add Your First Entry
            </button>
          )}
        </div>
      ) : (
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
                <span className={`knowledge-type-badge ${entry.type}`}>{entry.type}</span>
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
                  <span>Updated {formatDate(entry.updated_at)}</span>
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
                      <svg fill="none" stroke="currentColor" viewBox="0 0 24 24" aria-hidden="true">
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
      )}

      {/* Create/Edit Modal */}
      {showModal && (
        <div
          className="modal-overlay"
          onClick={() => setShowModal(false)}
          onKeyDown={(e) => {
            if (e.key === 'Escape') {
              setShowModal(false);
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
              <h2>Add Knowledge Entry</h2>
              <button
                type="button"
                className="modal-close"
                onClick={() => setShowModal(false)}
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
            <form onSubmit={handleCreateKnowledge}>
              <div className="modal-body">
                {formErrors._root && (
                  <div className="alert alert-error" role="alert">
                    {formErrors._root}
                  </div>
                )}

                <div className="form-group">
                  <label htmlFor="title">Title</label>
                  <input
                    id="title"
                    type="text"
                    value={formData.title}
                    onChange={(e) => setFormData({ ...formData, title: e.target.value })}
                    placeholder="Enter a title"
                    required
                  />
                  {formErrors.title && <div className="error">{formErrors.title}</div>}
                </div>

                <div className="form-group">
                  <label htmlFor="type">Type</label>
                  <select
                    id="type"
                    value={formData.type}
                    onChange={(e) =>
                      setFormData({ ...formData, type: e.target.value as KnowledgeType })
                    }
                  >
                    <option value="text">Text</option>
                    <option value="url">URL</option>
                  </select>
                  {formErrors.type && <div className="error">{formErrors.type}</div>}
                </div>

                <div className="form-group">
                  <label htmlFor="content">{formData.type === 'url' ? 'URL' : 'Content'}</label>
                  {formData.type === 'url' ? (
                    <input
                      id="content"
                      type="url"
                      value={formData.content}
                      onChange={(e) => setFormData({ ...formData, content: e.target.value })}
                      placeholder="https://example.com"
                      required
                    />
                  ) : (
                    <textarea
                      id="content"
                      value={formData.content}
                      onChange={(e) => setFormData({ ...formData, content: e.target.value })}
                      placeholder="Enter your content"
                      required
                    />
                  )}
                  {formErrors.content && <div className="error">{formErrors.content}</div>}
                </div>

                <div className="form-group">
                  <label htmlFor="tags">Tags (press Enter to add)</label>
                  <div className="tag-input-wrapper">
                    {formData.tags?.map((tag) => (
                      <span key={tag} className="tag-item">
                        {tag}
                        <button
                          type="button"
                          className="tag-remove"
                          onClick={() => handleRemoveTag(tag)}
                          aria-label={`Remove tag ${tag}`}
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
                              d="M6 18L18 6M6 6l12 12"
                            />
                          </svg>
                        </button>
                      </span>
                    ))}
                    <input
                      id="tags"
                      type="text"
                      value={tagInput}
                      onChange={(e) => setTagInput(e.target.value)}
                      onKeyDown={handleAddTag}
                      placeholder="Add tags..."
                    />
                  </div>
                  {formErrors.tags && <div className="error">{formErrors.tags}</div>}
                </div>
              </div>
              <div className="modal-footer">
                <button
                  type="button"
                  className="btn btn-secondary"
                  onClick={() => setShowModal(false)}
                  disabled={submitting}
                >
                  Cancel
                </button>
                <button type="submit" className="btn btn-primary" disabled={submitting}>
                  {submitting ? 'Creating...' : 'Create Entry'}
                </button>
              </div>
            </form>
          </div>
        </div>
      )}

      {/* View Entry Modal */}
      {selectedEntry && !showModal && (
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
              <div style={{ display: 'flex', alignItems: 'center', gap: '1rem' }}>
                <h2>{selectedEntry.title}</h2>
                <span className={`knowledge-type-badge ${selectedEntry.type}`}>
                  {selectedEntry.type}
                </span>
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
                <button
                  type="button"
                  className="btn btn-secondary"
                  onClick={() => handleRefreshKnowledge(selectedEntry.id)}
                  disabled={refreshing === selectedEntry.id}
                >
                  {refreshing === selectedEntry.id ? 'Refreshing...' : 'Refresh Content'}
                </button>
              )}
              <button
                type="button"
                className="btn btn-secondary"
                onClick={() => {
                  handleDeleteKnowledge(selectedEntry.id);
                  setSelectedEntry(null);
                }}
              >
                Delete
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
