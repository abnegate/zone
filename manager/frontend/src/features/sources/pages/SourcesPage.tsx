import { Badge, Button, EmptyState } from '@zone/ui';
import { useState } from 'react';
import { useSearchParams } from 'react-router-dom';
import PageBar from '../../../shared/components/PageBar/PageBar';
import PlusIcon from '../../../shared/components/PlusIcon/PlusIcon';
import { CreateSourceWizard } from '../components/CreateSourceWizard';
import { SourceDetail } from '../components/SourceDetail';
import { getSourceById, getSourceLabel } from '../config';
import { useSource, useSources } from '../hooks';
import type { Source } from '../types';
import './SourcesPage.css';

const SKELETON_ROWS = [1, 2, 3];

function stopRowActivation(event: { stopPropagation: () => void }) {
  event.stopPropagation();
}

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
  const [searchParams, setSearchParams] = useSearchParams();
  const sourceParam = searchParams.get('source') || null;
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
  const {
    source: selected,
    loading: sourceLoading,
    error: sourceError,
    updateSource: updateSelected,
    verifySource: verifySelected,
  } = useSource(sourceParam);
  const [showCreateModal, setShowCreateModal] = useState(false);
  const [verifying, setVerifying] = useState<string | null>(null);
  const [operationError, setOperationError] = useState<string | null>(null);

  const showEditor =
    Boolean(sourceParam) && selected?.id === sourceParam && !sourceError && !sourceLoading;
  const showSpinner = Boolean(sourceParam) && !showEditor && !sourceError;
  const showAddSource = !showSpinner && !showEditor;
  const displayError = error || operationError || (sourceParam && !showEditor ? sourceError : null);
  const unloaded = error !== null && sources.length === 0;

  const openSource = (id: string) => {
    const next = new URLSearchParams(searchParams);
    next.set('source', id);
    setSearchParams(next);
  };

  const closeSource = () => {
    if (!searchParams.has('source')) return;
    const next = new URLSearchParams(searchParams);
    next.delete('source');
    setSearchParams(next, { replace: true });
  };

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

  return (
    <div className="page page--workspace sources-page">
      <PageBar
        title="Sources"
        subtitle="Connect repositories, calendars, email, and other data sources"
      >
        {showAddSource ? (
          <Button onClick={() => setShowCreateModal(true)}>
            <PlusIcon />
            Add source
          </Button>
        ) : undefined}
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

        {showSpinner ? (
          <div className="source-details-loading" role="status">
            Loading source
          </div>
        ) : showEditor && selected ? (
          <SourceDetail
            key={selected.id}
            source={selected}
            onClose={closeSource}
            onSaved={refresh}
            updateSource={updateSelected}
            verifySource={verifySelected}
          />
        ) : loading ? (
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
                      tabIndex={0}
                      onClick={() => openSource(source.id)}
                      onKeyDown={(event) => {
                        if (event.key !== 'Enter' && event.key !== ' ') return;
                        event.preventDefault();
                        openSource(source.id);
                      }}
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
                          onClick={stopRowActivation}
                          onKeyDown={stopRowActivation}
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
                        <div
                          className="source-actions"
                          role="group"
                          onClick={stopRowActivation}
                          onKeyDown={stopRowActivation}
                        >
                          <Button
                            variant="ghost"
                            size="sm"
                            onClick={(event) => {
                              event.stopPropagation();
                              handleVerify(source.id);
                            }}
                            loading={verifying === source.id}
                          >
                            {verifying === source.id ? 'Verifying...' : 'Verify'}
                          </Button>
                          <Button
                            variant="ghost"
                            size="sm"
                            onClick={(event) => {
                              event.stopPropagation();
                              handleToggleActive(source);
                            }}
                          >
                            {source.is_active ? 'Disable' : 'Enable'}
                          </Button>
                          <Button
                            className="source-delete"
                            variant="ghost"
                            size="sm"
                            onClick={(event) => {
                              event.stopPropagation();
                              handleDelete(source.id);
                            }}
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
