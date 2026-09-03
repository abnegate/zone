import { Badge, Button, EmptyState } from '@zone/ui';
import { useState } from 'react';
import { CreateSourceWizard } from '../components/CreateSourceWizard';
import { getSourceLabel } from '../config';
import { useSources } from '../hooks';
import type { Source, SourceType } from '../types';
import './SourcesPage.css';

const sourceTypeVariants: Record<
  SourceType,
  'default' | 'secondary' | 'info' | 'success' | 'warning' | 'destructive'
> = {
  github: 'default',
  gitlab: 'warning',
  filesystem: 'info',
  slack: 'secondary',
  discord: 'secondary',
  ical: 'success',
  imap: 'secondary',
  web: 'info',
  text: 'secondary',
};

function SourceTypeBadge({ type }: { type: SourceType }) {
  return <Badge variant={sourceTypeVariants[type] || 'secondary'}>{getSourceLabel(type)}</Badge>;
}

function SourceStatusBadge({ source }: { source: Source }) {
  if (!source.is_active) {
    return <Badge variant="secondary">Inactive</Badge>;
  }
  if (source.last_error) {
    return <Badge variant="destructive">Error</Badge>;
  }
  if (source.last_verified_at) {
    return <Badge variant="success">Verified</Badge>;
  }
  return <Badge variant="warning">Unverified</Badge>;
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
        setOperationError(result.message || 'Verification failed');
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
    <div className="page page--workspace sources-page">
      <header className="sources-header">
        <div className="sources-header-copy">
          <h1>Sources</h1>
          <p>Connect repositories, calendars, email, and other data sources</p>
        </div>
        <Button onClick={() => setShowCreateModal(true)}>+ Add Source</Button>
      </header>

      {displayError && (
        <div className="sources-banner sources-banner--error" role="alert">
          {displayError}
        </div>
      )}

      {loading ? (
        <div className="sources-workspace">
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
        </div>
      ) : sources.length === 0 ? (
        <EmptyState
          className="sources-empty"
          icon={
            <svg
              viewBox="0 0 24 24"
              fill="none"
              stroke="currentColor"
              strokeWidth="1.5"
              width="48"
              height="48"
            >
              <path d="M3 7v10a2 2 0 002 2h14a2 2 0 002-2V9a2 2 0 00-2-2h-6l-2-2H5a2 2 0 00-2 2z" />
              <path d="M12 11v6m-3-3h6" />
            </svg>
          }
          title="No sources configured"
          description="Add code repositories, calendars, email inboxes, web URLs, or text content"
          action={<Button onClick={() => setShowCreateModal(true)}>Add Source</Button>}
        />
      ) : (
        <div className="sources-workspace">
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
                    size="sm"
                    onClick={() => handleVerify(source.id)}
                    loading={verifying === source.id}
                  >
                    {verifying === source.id ? 'Verifying...' : 'Verify'}
                  </Button>
                  <Button
                    variant={source.is_active ? 'secondary' : 'default'}
                    size="sm"
                    onClick={() => handleToggleActive(source)}
                  >
                    {source.is_active ? 'Disable' : 'Enable'}
                  </Button>
                  <Button variant="destructive" size="sm" onClick={() => handleDelete(source.id)}>
                    Delete
                  </Button>
                </div>
              </div>
            ))}
          </div>
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
