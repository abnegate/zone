import { Button, EmptyState } from '@zone/ui';
import { type ReactElement, useState } from 'react';
import { useLocation, useNavigate, useSearchParams } from 'react-router-dom';
import { getSourceById, getSourceLabel } from '../config';
import { useSource, useSources } from '../hooks';
import type { Source } from '../types';
import { CreateSourceWizard } from './CreateSourceWizard';
import { SourceDetail } from './SourceDetail';
import { COLUMNS, READ_COLUMNS, SourceSkeleton } from './SourceSkeleton';
import { SourceStatusBadge } from './SourceStatusBadge';
import './SourcesSection.css';

const SOURCE_PARAMETER = 'source';

interface EditorEntry {
  listed: boolean;
}

const LISTED_ENTRY: EditorEntry = { listed: true };

function openedFromList(state: unknown): boolean {
  return (state as Partial<EditorEntry> | null)?.listed === true;
}

function stopRowActivation(event: { stopPropagation: () => void }): void {
  event.stopPropagation();
}

const READ_ONLY_HINT = "Viewers can't change sources.";

const EMPTY_DESCRIPTION =
  'Add code repositories, calendars, email inboxes, web URLs, or text content';

const READ_ONLY_EMPTY_DESCRIPTION =
  'Workspace members can add code repositories, calendars, email inboxes, web URLs, or text content';

function formatVerified(at: string): string {
  return new Date(at).toLocaleDateString(undefined, {
    year: 'numeric',
    month: 'short',
    day: 'numeric',
  });
}

export function SourcesSection({ readOnly = false }: { readOnly?: boolean }): ReactElement {
  const [searchParams, setSearchParams] = useSearchParams();
  const location = useLocation();
  const navigate = useNavigate();
  const selectedId = searchParams.get(SOURCE_PARAMETER) || null;
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
  } = useSource(selectedId);
  const [showCreateModal, setShowCreateModal] = useState(false);
  const [verifying, setVerifying] = useState<string | null>(null);
  const [operationError, setOperationError] = useState<string | null>(null);

  const showEditor =
    Boolean(selectedId) && selected?.id === selectedId && !sourceError && !sourceLoading;
  const showSpinner = Boolean(selectedId) && !showEditor && !sourceError;
  const showAddSource = !readOnly && !showSpinner && !showEditor;
  const columns = readOnly ? READ_COLUMNS : COLUMNS;
  const displayError = error || operationError || (selectedId && !showEditor ? sourceError : null);
  const unloaded = error !== null && sources.length === 0;

  const openSource = (id: string): void => {
    const next = new URLSearchParams(searchParams);
    next.set(SOURCE_PARAMETER, id);
    setSearchParams(
      next,
      searchParams.has(SOURCE_PARAMETER)
        ? { replace: true, state: location.state }
        : { state: LISTED_ENTRY }
    );
  };

  const closeSource = (): void => {
    if (!searchParams.has(SOURCE_PARAMETER)) return;
    if (openedFromList(location.state)) {
      navigate(-1);
      return;
    }
    const next = new URLSearchParams(searchParams);
    next.delete(SOURCE_PARAMETER);
    setSearchParams(next, { replace: true });
  };

  const handleVerify = async (sourceId: string): Promise<void> => {
    setVerifying(sourceId);
    setOperationError(null);
    try {
      const result = await verifySource(sourceId);
      if (!result.verified) {
        setOperationError(result.message || 'Verification failed');
      }
    } catch (failure) {
      setOperationError(failure instanceof Error ? failure.message : 'Failed to verify source');
    } finally {
      setVerifying(null);
    }
  };

  const handleDelete = async (sourceId: string): Promise<void> => {
    if (!window.confirm('Are you sure you want to delete this source?')) return;

    setOperationError(null);
    try {
      await deleteSource(sourceId);
    } catch (failure) {
      setOperationError(failure instanceof Error ? failure.message : 'Failed to delete source');
    }
  };

  const handleToggleActive = async (source: Source): Promise<void> => {
    setOperationError(null);
    try {
      await updateSource(source.id, { is_active: !source.is_active });
    } catch (failure) {
      setOperationError(failure instanceof Error ? failure.message : 'Failed to update source');
    }
  };

  return (
    <div className="sources-section">
      <div className="section-row">
        <div className="section-row-copy">
          <h2 className="section-title">Sources</h2>
          <p className="section-description">
            Connect repositories, calendars, email, and other data sources
          </p>
        </div>
        {showAddSource && (
          <div className="section-row-actions">
            <Button size="sm" onClick={() => setShowCreateModal(true)}>
              Add source
            </Button>
          </div>
        )}
      </div>

      {readOnly && <p className="sources-hint">{READ_ONLY_HINT}</p>}

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
          readOnly={readOnly}
        />
      ) : loading ? (
        <SourceSkeleton columns={columns} />
      ) : unloaded ? null : sources.length === 0 ? (
        <EmptyState
          icon={
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5">
              <path d="M3 7v10a2 2 0 002 2h14a2 2 0 002-2V9a2 2 0 00-2-2h-6l-2-2H5a2 2 0 00-2 2z" />
              <path d="M12 11v6m-3-3h6" />
            </svg>
          }
          title="No sources configured"
          description={readOnly ? READ_ONLY_EMPTY_DESCRIPTION : EMPTY_DESCRIPTION}
          action={
            readOnly ? undefined : (
              <Button onClick={() => setShowCreateModal(true)}>Add source</Button>
            )
          }
        />
      ) : (
        <div className="sources-table-wrapper">
          <table className="sources-table" aria-label="Sources">
            <thead>
              <tr>
                {columns.map((column) => (
                  <th key={column}>{column}</th>
                ))}
              </tr>
            </thead>
            <tbody>
              {sources.map((source) => {
                const definition = getSourceById(source.source_type);
                return (
                  <tr
                    key={source.id}
                    className={`source-card ${source.is_active ? '' : 'source-inactive'}`.trim()}
                    onClick={() => openSource(source.id)}
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
                          <button
                            type="button"
                            className="source-name"
                            onClick={(event) => {
                              event.stopPropagation();
                              openSource(source.id);
                            }}
                          >
                            {source.name}
                          </button>
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
                      <span className="source-provider">{getSourceLabel(source.source_type)}</span>
                    </td>
                    <td>
                      <SourceStatusBadge source={source} />
                    </td>
                    <td className="source-url-cell">
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
                          ? formatVerified(source.last_verified_at)
                          : 'Never verified'}
                      </span>
                    </td>
                    {!readOnly && (
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
                    )}
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>
      )}

      {!readOnly && (
        <CreateSourceWizard
          isOpen={showCreateModal}
          onClose={() => setShowCreateModal(false)}
          createSource={createSource}
        />
      )}
    </div>
  );
}
