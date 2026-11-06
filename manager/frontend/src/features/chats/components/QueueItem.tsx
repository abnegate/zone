import { Button } from '@zone/ui';
import { type ReactElement, useEffect, useRef, useState } from 'react';
import PencilIcon from '../../../shared/components/PencilIcon/PencilIcon';
import TrashIcon from '../../../shared/components/TrashIcon/TrashIcon';
import type { QueuedMessage } from '../types';
import './QueueItem.css';

interface QueueItemProps {
  message: QueuedMessage;
  held: boolean;
  onInterrupt: (id: string) => void;
  onEdit: (id: string, content: string) => void;
  onRemove: (id: string) => void;
}

export function QueueItem({
  message,
  held,
  onInterrupt,
  onEdit,
  onRemove,
}: QueueItemProps): ReactElement {
  const [draft, setDraft] = useState<string | null>(null);
  const editing = draft !== null;
  const editor = useRef<HTMLTextAreaElement>(null);
  useEffect(() => {
    const field = editor.current;
    if (!editing || !field) return;
    field.focus();
    field.setSelectionRange(field.value.length, field.value.length);
  }, [editing]);
  const changed = editing && draft.trim().length > 0 && draft !== message.content;

  const save = () => {
    if (!changed) return;
    onEdit(message.id, draft);
    setDraft(null);
  };

  return (
    <div
      className="message message-user queue-item"
      role="group"
      aria-label="Queued message"
      data-testid="queued-message"
    >
      {editing ? (
        <textarea
          ref={editor}
          className="queue-item-editor"
          aria-label="Edit queued message"
          value={draft}
          onChange={(event) => setDraft(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === 'Enter' && !event.shiftKey) {
              event.preventDefault();
              save();
            } else if (event.key === 'Escape') {
              setDraft(null);
            }
          }}
          rows={Math.min(8, draft.split('\n').length + 1)}
        />
      ) : (
        <div className="message-content queue-item-content">{message.content}</div>
      )}
      <div className="queue-item-footer">
        {editing ? (
          <>
            <Button type="button" variant="ghost" size="sm" onClick={() => setDraft(null)}>
              Cancel
            </Button>
            <Button type="button" variant="secondary" size="sm" onClick={save} disabled={!changed}>
              Save
            </Button>
          </>
        ) : (
          <>
            <Button
              type="button"
              variant="ghost"
              size="icon"
              aria-label="Remove queued message"
              title="Remove"
              onClick={() => onRemove(message.id)}
            >
              <TrashIcon />
            </Button>
            <Button
              type="button"
              variant="ghost"
              size="icon"
              aria-label="Edit queued message"
              title="Edit"
              onClick={() => setDraft(message.content)}
            >
              <PencilIcon />
            </Button>
            <Button
              type="button"
              variant="secondary"
              size="sm"
              onClick={() => onInterrupt(message.id)}
            >
              {held ? 'Send now' : 'Interrupt'}
            </Button>
          </>
        )}
      </div>
    </div>
  );
}
