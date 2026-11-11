import { useEffect, useRef } from 'react';
import type { Source } from '../../../types';

export const NEW_SOURCE_OPTION = 'new-source';

interface SourceStepProps {
  sources: Source[];
  loading: boolean;
  selectedId: string;
  onSelect: (id: string) => void;
  canCreate: boolean;
  onCreate: () => void;
  focusId: string | null;
}

export function SourceStep({
  sources,
  loading,
  selectedId,
  onSelect,
  canCreate,
  onCreate,
  focusId,
}: SourceStepProps) {
  const options = useRef(new Map<string, HTMLButtonElement>());

  useEffect(() => {
    if (focusId) options.current.get(focusId)?.focus();
  }, [focusId]);

  return (
    <div className="wizard-step-content">
      <p className="wizard-step-intro">
        Link a data source to this project. Sources provide code repositories, calendars, or other
        data for your project.
      </p>
      {loading ? (
        <div className="wizard-loading">Loading sources...</div>
      ) : sources.length === 0 && !canCreate ? (
        <div className="wizard-empty-state">
          <p>No sources available.</p>
          <p className="wizard-empty-hint">
            You can add sources in Settings › Workspace › Sources and link them later.
          </p>
        </div>
      ) : (
        <div className="source-selection-grid">
          <button
            type="button"
            className={`source-selection-option ${!selectedId ? 'selected' : ''}`}
            aria-pressed={!selectedId}
            onClick={() => onSelect('')}
          >
            <div className="source-selection-icon none">
              <svg
                viewBox="0 0 24 24"
                fill="none"
                stroke="currentColor"
                strokeWidth="2"
                aria-hidden="true"
              >
                <circle cx="12" cy="12" r="10" />
                <path d="M8 12h8" />
              </svg>
            </div>
            <div className="source-selection-info">
              <span className="source-selection-name">No Source</span>
              <span className="source-selection-desc">Create project without a source</span>
            </div>
          </button>
          {sources.map((source) => (
            <button
              key={source.id}
              ref={(element) => {
                if (element) options.current.set(source.id, element);
                else options.current.delete(source.id);
              }}
              type="button"
              className={`source-selection-option ${selectedId === source.id ? 'selected' : ''}`}
              aria-pressed={selectedId === source.id}
              onClick={() => onSelect(source.id)}
            >
              <div className={`source-selection-icon ${source.source_type}`}>
                <span className="source-type-initial">
                  {source.source_type.charAt(0).toUpperCase()}
                </span>
              </div>
              <div className="source-selection-info">
                <span className="source-selection-name">{source.name}</span>
                <span className="source-selection-desc">{source.source_type}</span>
              </div>
            </button>
          ))}
          {canCreate && (
            <button
              ref={(element) => {
                if (element) options.current.set(NEW_SOURCE_OPTION, element);
                else options.current.delete(NEW_SOURCE_OPTION);
              }}
              type="button"
              className="source-selection-option source-selection-new"
              onClick={onCreate}
              aria-haspopup="dialog"
            >
              <div className="source-selection-icon new">
                <svg
                  viewBox="0 0 24 24"
                  fill="none"
                  stroke="currentColor"
                  strokeWidth="2"
                  strokeLinecap="round"
                  aria-hidden="true"
                >
                  <path d="M12 5v14M5 12h14" />
                </svg>
              </div>
              <div className="source-selection-info">
                <span className="source-selection-name">New source</span>
                <span className="source-selection-desc">Connect a repository or folder</span>
              </div>
            </button>
          )}
        </div>
      )}
    </div>
  );
}

export default SourceStep;
