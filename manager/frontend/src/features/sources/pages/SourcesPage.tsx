import { useState } from 'react';
import { Button } from '../../../components';
import { useSources } from '../hooks';
import {
  getSourceBadgeColor,
  getSourceLabel,
} from '../config';
import type { Source, SourceType } from '../types';
import { CreateSourceWizard } from '../components/CreateSourceWizard';
import './SourcesPage.css';

function SourceTypeBadge({ type }: { type: SourceType }) {
  return (
    <span className={`source-type-badge ${getSourceBadgeColor(type)}`}>{getSourceLabel(type)}</span>
  );
}

function SourceStatusBadge({ source }: { source: Source }) {
  if (!source.is_active) {
    return <span className="source-status-badge badge-gray">Inactive</span>;
  }
  if (source.last_error) {
    return <span className="source-status-badge badge-red">Error</span>;
  }
  if (source.last_verified_at) {
    return <span className="source-status-badge badge-green">Verified</span>;
  }
  return <span className="source-status-badge badge-yellow">Unverified</span>;
}

export default function SourcesPage() {
  const { sources, loading, error, createSource, updateSource, deleteSource, verifySource } =
    useSources();
  const [showCreateModal, setShowCreateModal] = useState(false);
  const [verifying, setVerifying] = useState<string | null>(null);
  const [operationError, setOperationError] = useState<string | null>(null);

  const handleSourceCreated = () => {
    // The hook automatically updates the sources list
  };

  const handleVerify = async (sourceId: string) => {
    setVerifying(sourceId);
    setOperationError(null);
    try {
      const result = await verifySource(sourceId);
      if (!result.success) {
        // Error handling could be improved here
        console.error(`Verification failed: ${result.message}`);
      }
    } catch (err) {
      setOperationError(err instanceof Error ? err.message : 'Failed to verify source');
    } finally {
      setVerifying(null);
    }
  };

  const handleDelete = async (sourceId: string) => {
    if (!window.confirm('Are you sure you want to delete this source?')) return;

    setOperationError(null);
    try {
      await deleteSource(sourceId);
    } catch (err) {
      setOperationError(err instanceof Error ? err.message : 'Failed to delete source');
    }
  };

  const handleToggleActive = async (source: Source) => {
    setOperationError(null);
    try {
      await updateSource(source.id, { is_active: !source.is_active });
    } catch (err) {
      setOperationError(err instanceof Error ? err.message : 'Failed to update source');
    }
  };

  // Combine errors from hook and operations
  const displayError = error || operationError;

  return (
    <div className="page sources-page">
      <header className="page-header">
        <div className="header-content">
          <h1>Sources</h1>
          <p className="subtitle">Connect repositories, calendars, email, and other data sources</p>
        </div>
        <Button variant="primary" onClick={() => setShowCreateModal(true)}>
          + Add Source
        </Button>
      </header>

      {displayError && <div className="error-banner">{displayError}</div>}

      {loading ? (
        <div className="sources-list">
          {[1, 2, 3].map((i) => (
            <div key={i} className="source-card skeleton-card">
              <div className="skeleton-header">
                <div className="skeleton skeleton-title" />
                <div className="skeleton skeleton-badge" />
              </div>
              <div className="skeleton skeleton-text" />
              <div className="skeleton skeleton-text short" />
              <div className="skeleton-actions">
                <div className="skeleton skeleton-btn" />
                <div className="skeleton skeleton-btn" />
              </div>
            </div>
          ))}
        </div>
      ) : sources.length === 0 ? (
        <div className="empty-state">
          <p>No sources configured. Add a source to get started!</p>
          <p className="empty-hint">
            Sources can be code repositories, calendars, email inboxes, web URLs, or text content.
          </p>
        </div>
      ) : (
        <div className="sources-list">
          {sources.map((source) => (
            <div
              key={source.id}
              className={`source-card ${!source.is_active ? 'source-inactive' : ''}`}
            >
              <div className="source-card-header">
                <h3>{source.name}</h3>
                <div className="source-badges">
                  <SourceTypeBadge type={source.source_type} />
                  <SourceStatusBadge source={source} />
                </div>
              </div>

              {source.description && <p className="source-description">{source.description}</p>}

              <div className="source-url">
                <a href={source.url} target="_blank" rel="noopener noreferrer">
                  {source.url}
                </a>
              </div>

              {source.last_error && <div className="source-error">{source.last_error}</div>}

              <div className="source-meta">
                {source.last_verified_at && (
                  <span>Verified: {new Date(source.last_verified_at).toLocaleDateString()}</span>
                )}
              </div>

              <div className="source-actions">
                <Button
                  variant="secondary"
                  onClick={() => handleVerify(source.id)}
                  loading={verifying === source.id}
                >
                  {verifying === source.id ? 'Verifying...' : 'Verify'}
                </Button>
                <Button
                  variant={source.is_active ? 'secondary' : 'primary'}
                  onClick={() => handleToggleActive(source)}
                >
                  {source.is_active ? 'Disable' : 'Enable'}
                </Button>
                <Button variant="danger" onClick={() => handleDelete(source.id)}>
                  Delete
                </Button>
              </div>
            </div>
          ))}
        </div>
      )}

      <CreateSourceWizard
        isOpen={showCreateModal}
        onClose={() => setShowCreateModal(false)}
        onCreated={handleSourceCreated}
        createSource={createSource}
      />
    </div>
  );
}
