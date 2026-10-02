import { Badge, Button, EmptyState } from '@zone/ui';
import { useState } from 'react';
import PageBar from '../../../shared/components/PageBar/PageBar';
import PlusIcon from '../../../shared/components/PlusIcon/PlusIcon';
import { CreateSourceWizard } from '../components/CreateSourceWizard';
import { getSourceById, getSourceLabel } from '../config';
import { useSources } from '../hooks';
import type { Source } from '../types';
import './SourcesPage.css';

const SKELETON_ROWS = [1, 2, 3];

function SourceStatusBadge({ source }: { source: Source }) {
  if (!source.is_active) {
    return (
      <Badge className="source-status" variant="neutral">
        Inactive
      </Badge>
    );
  }
  if (source.last_error) {
    return (
      <Badge className="source-status" variant="destructive">
        Error
      </Badge>
    );
  }
  if (source.last_verified_at) {
    return (
      <Badge className="source-status" variant="success">
        Verified
      </Badge>
    );
  }
  return (
    <Badge className="source-status" variant="warning">
      Unverified
    </Badge>
  );
}

function formatVerified(at: string): string {
  return new Date(at).toLocaleDateString(undefined, {
    year: 'numeric',
    month: 'short',
    day: 'numeric',
  });
}

function SourceSkeleton() {
  return (
    <div className="sources-table-wrapper" aria-hidden="true">
      <table className="sources-table">
        <thead>
          <tr>
            <th>Name</th>
            <th>Type</th>
            <th>Status</th>
            <th>URL</th>
            <th>Verified</th>
            <th>Actions</th>
          </tr>
        </thead>
        <tbody>
          {SKELETON_ROWS.map((row) => (
            <tr key={row} className="source-card skeleton-card">
              <td>
                <div className="skeleton skeleton-title" />
              </td>
              <td>
                <div className="skeleton skeleton-text short" />
              </td>
              <td>
                <div className="skeleton skeleton-badge" />
              </td>
              <td>
                <div className="skeleton skeleton-text" />
              </td>
              <td>
                <div className="skeleton skeleton-text short" />
              </td>
              <td>
                <div className="skeleton skeleton-badge" />
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

export default function SourcesPage() {
  const {
    sources,
    loading,
    error,
    createSource,
    updateSource,
    deleteSource,
    verifySource,
    refresh,
  } = useSources();
  const [showCreateModal, setShowCreateModal] = useState(false);
  const [verifying, setVerifying] = useState<string | null>(null);
  const [operationError, setOperationError] = useState<string | null>(null);

  const handleVerify = async (sourceId: string) => {
    setVerifying(sourceId);
    setOperationError(null);
    try {
      const result = await verifySource(sourceId);
      if (!result.verified) {
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

  const displayError = error || operationError;
  const unloaded = error !== null && sources.length === 0;

  return (
    <div className="page page--workspace sources-page">
      <PageBar
        title="Sources"
        subtitle="Connect repositories, calendars, email, and other data sources"
      >
        <Button onClick={() => setShowCreateModal(true)}>
          <PlusIcon />
          Add source
        </Button>
      </PageBar>

      <div className="page-body sources-body">
        {displayError && (
          <div className="sources-banner sources-banner--error" role="alert">
            <span>{displayError}</span>
            {error && (
              <Button variant="ghost" size="sm" onClick={() => refresh()}>
                Retry
              </Button>
            )}
          </div>
        )}

        {loading ? (
          <SourceSkeleton />
        ) : unloaded ? null : sources.length === 0 ? (
          <EmptyState
            className="sources-empty"
            icon={
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5">
                <path d="M3 7v10a2 2 0 002 2h14a2 2 0 002-2V9a2 2 0 00-2-2h-6l-2-2H5a2 2 0 00-2 2z" />
                <path d="M12 11v6m-3-3h6" />
              </svg>
            }
            title="No sources configured"
            description="Add code repositories, calendars, email inboxes, web URLs, or text content"
            action={<Button onClick={() => setShowCreateModal(true)}>Add source</Button>}
          />
        ) : (
          <div className="sources-table-wrapper">
            <table className="sources-table" aria-label="Sources">
              <thead>
                <tr>
                  <th>Name</th>
                  <th>Type</th>
                  <th>Status</th>
                  <th>URL</th>
                  <th>Verified</th>
                  <th>Actions</th>
                </tr>
              </thead>
              <tbody>
                {sources.map((source) => {
                  const definition = getSourceById(source.source_type);
                  return (
                    <tr
                      key={source.id}
                      className={`source-card ${source.is_active ? '' : 'source-inactive'}`.trim()}
                    >
                      <td>
                        <div className="source-name-cell">
                          {definition && (
                            <span
                              className={`source-provider-icon ${definition.iconWrapperClass}`}
                              aria-hidden="true"
                            >
                              {definition.icon}
                            </span>
                          )}
                          <div className="source-name-copy">
                            <span className="source-name">{source.name}</span>
                            {source.description && (
                              <span className="source-description">{source.description}</span>
                            )}
                            {source.last_error && (
                              <span className="source-error">{source.last_error}</span>
                            )}
                          </div>
                        </div>
                      </td>
                      <td>
                        <span className="source-provider">
                          {getSourceLabel(source.source_type)}
                        </span>
                      </td>
                      <td>
                        <SourceStatusBadge source={source} />
                      </td>
                      <td>
                        <a
                          className="source-url"
                          href={source.url}
                          target="_blank"
                          rel="noopener noreferrer"
                        >
                          {source.url}
                        </a>
                      </td>
                      <td>
                        <span className="source-meta">
                          {source.last_verified_at
                            ? `Verified ${formatVerified(source.last_verified_at)}`
                            : 'Never verified'}
                        </span>
                      </td>
                      <td>
                        <div className="source-actions">
                          <Button
                            variant="ghost"
                            size="sm"
                            onClick={() => handleVerify(source.id)}
                            loading={verifying === source.id}
                          >
                            {verifying === source.id ? 'Verifying...' : 'Verify'}
                          </Button>
                          <Button
                            variant="ghost"
                            size="sm"
                            onClick={() => handleToggleActive(source)}
                          >
                            {source.is_active ? 'Disable' : 'Enable'}
                          </Button>
                          <Button
                            className="source-delete"
                            variant="ghost"
                            size="sm"
                            onClick={() => handleDelete(source.id)}
                          >
                            Delete
                          </Button>
                        </div>
                      </td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          </div>
        )}
      </div>

      <CreateSourceWizard
        isOpen={showCreateModal}
        onClose={() => setShowCreateModal(false)}
        onCreated={() => undefined}
        createSource={createSource}
      />
    </div>
  );
}
