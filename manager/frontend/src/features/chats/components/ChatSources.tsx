import { useEffect, useId, useRef, useState } from 'react';
import type { Source } from '../../sources/types';
import type { ChatSource } from '../types';

interface ChatSourcesProps {
  attached: ChatSource[];
  available: Source[];
  loading: boolean;
  error: string | null;
  onChange: (sourceIds: string[]) => Promise<void>;
}

const KIND_LABELS: Record<string, string> = {
  github: 'GitHub',
  gitlab: 'GitLab',
  filesystem: 'Folder',
  notion: 'Notion',
  text: 'Text',
  web: 'Web',
  imap: 'Mail',
  ical: 'Calendar',
  slack: 'Slack',
  discord: 'Discord',
};

function kindLabel(kind: string): string {
  return KIND_LABELS[kind] ?? kind;
}

/**
 * The sources this chat's retrieval is confined to, chosen from the
 * workspace's sources. Nothing attached searches the whole workspace, and the
 * chip says so rather than pretending the empty set means nothing.
 */
export function ChatSources({ attached, available, loading, error, onChange }: ChatSourcesProps) {
  const [open, setOpen] = useState(false);
  const [saving, setSaving] = useState(false);
  const bar = useRef<HTMLDivElement>(null);
  const listId = useId();

  useEffect(() => {
    if (!open) return;
    const close = (event: MouseEvent) => {
      if (bar.current && !bar.current.contains(event.target as Node)) setOpen(false);
    };
    const dismiss = (event: KeyboardEvent) => {
      if (event.key === 'Escape') setOpen(false);
    };
    document.addEventListener('mousedown', close);
    document.addEventListener('keydown', dismiss);
    return () => {
      document.removeEventListener('mousedown', close);
      document.removeEventListener('keydown', dismiss);
    };
  }, [open]);

  const attachedIds = new Set(attached.map((source) => source.id));

  const apply = async (ids: string[]) => {
    setSaving(true);
    try {
      await onChange(ids);
    } finally {
      setSaving(false);
    }
  };

  const toggle = (id: string) => {
    const next = attachedIds.has(id)
      ? attached.filter((source) => source.id !== id).map((source) => source.id)
      : [...attached.map((source) => source.id), id];
    void apply(next);
  };

  return (
    <div className="chat-sources-bar" ref={bar} data-testid="chat-sources">
      <button
        type="button"
        className={`chat-sources-toggle${attached.length > 0 ? ' is-attached' : ''}`}
        onClick={() => setOpen((current) => !current)}
        aria-expanded={open}
        aria-controls={listId}
        aria-label={
          attached.length > 0 ? `Sources: ${attached.length} attached` : 'Sources: whole workspace'
        }
        title={
          attached.length > 0
            ? 'Replies search only the attached sources'
            : 'Replies search the whole workspace; attach sources to narrow them'
        }
        disabled={loading}
      >
        <svg
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          strokeWidth="1.75"
          width="14"
          height="14"
          aria-hidden="true"
        >
          <path d="M12 3 3 7.5 12 12l9-4.5L12 3z" />
          <path d="M3 12l9 4.5 9-4.5M3 16.5 12 21l9-4.5" />
        </svg>
        <span>Sources</span>
        {attached.length > 0 ? (
          <span className="chat-sources-count">{attached.length}</span>
        ) : (
          <span className="chat-sources-scope">all</span>
        )}
      </button>

      {attached.map((source) => (
        <span key={source.id} className="chat-source-chip" data-testid="chat-source-chip">
          <span className="chat-source-chip-kind">{kindLabel(source.source_type)}</span>
          <span className="chat-source-chip-name">{source.name}</span>
          <button
            type="button"
            className="chat-source-chip-remove"
            onClick={() => toggle(source.id)}
            disabled={saving}
            aria-label={`Detach ${source.name}`}
          >
            ×
          </button>
        </span>
      ))}

      {open ? (
        <div className="chat-sources-popover" id={listId} role="group" aria-label="Attach sources">
          <div className="chat-sources-popover-header">
            <strong>Search only these sources</strong>
            <span>
              {attached.length > 0
                ? `Replies look in ${attached.length} of ${available.length} sources.`
                : 'Nothing attached searches the whole workspace.'}
            </span>
          </div>
          {available.length === 0 ? (
            <p className="chat-sources-empty">
              No sources in this workspace yet. Connect a repository, folder or page under Sources
              first.
            </p>
          ) : (
            <div className="chat-sources-options">
              {available.map((source) => (
                <label key={source.id} className="chat-sources-option">
                  <input
                    type="checkbox"
                    checked={attachedIds.has(source.id)}
                    onChange={() => toggle(source.id)}
                    disabled={saving}
                  />
                  <span className="chat-sources-option-name">{source.name}</span>
                  <span className="chat-sources-option-kind">{kindLabel(source.source_type)}</span>
                </label>
              ))}
            </div>
          )}
          {error ? <p className="chat-sources-error">{error}</p> : null}
          <div className="chat-sources-popover-footer">
            <span>{saving ? 'Saving…' : 'Saved per chat'}</span>
            {attached.length > 0 ? (
              <button
                type="button"
                className="chat-sources-clear"
                onClick={() => void apply([])}
                disabled={saving}
              >
                Detach all
              </button>
            ) : null}
          </div>
        </div>
      ) : null}
    </div>
  );
}
