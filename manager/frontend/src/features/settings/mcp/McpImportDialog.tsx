import { Button, Modal } from '@zone/ui';
import { useId, useMemo, useState } from 'react';
import { messageOf } from './failure';
import { parseMcpDocument } from './importDocument';
import type { CreateMcpServerRequest } from './types';
import { REFERENCE } from './validation';
import './McpImportDialog.css';

export const IMPORT_HINT = `Paste a document with an mcpServers object, or a bare map of servers. Replace ${REFERENCE}…} references with their values first.`;
export const IMPORT_FAILED = 'Failed to import the MCP servers';

export function importLabel(count: number): string {
  return count === 1 ? 'Import 1 server' : `Import ${count} servers`;
}

interface McpImportDialogProps {
  commands: boolean;
  onImport: (requests: CreateMcpServerRequest[]) => Promise<void>;
  onClose: () => void;
}

export function McpImportDialog({ commands, onImport, onClose }: McpImportDialogProps) {
  const base = useId();
  const textId = `${base}-json`;
  const hintId = `${base}-hint`;
  const addId = `${base}-add`;
  const skippedId = `${base}-skipped`;
  const [text, setText] = useState('');
  const [pending, setPending] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const document = useMemo(() => parseMcpDocument(text, commands), [text, commands]);
  const count = document.requests.length;

  const submit = async () => {
    if (pending || count === 0) {
      return;
    }
    setPending(true);
    setError(null);
    try {
      await onImport(document.requests);
    } catch (failure) {
      setError(messageOf(failure, IMPORT_FAILED));
    } finally {
      setPending(false);
    }
  };

  return (
    <Modal isOpen onClose={pending ? undefined : onClose} title="Import MCP servers" size="md">
      <div className="mcp-import">
        <div className="form-group">
          <label htmlFor={textId}>mcpServers JSON</label>
          <textarea
            id={textId}
            className="mcp-import-json"
            rows={10}
            value={text}
            spellCheck={false}
            autoComplete="off"
            disabled={pending}
            aria-describedby={hintId}
            onChange={(event) => setText(event.target.value)}
          />
          <p id={hintId} className="form-hint">
            {IMPORT_HINT}
          </p>
        </div>
        {count > 0 && (
          <div className="mcp-import-group">
            <p id={addId} className="form-label">
              Servers to add
            </p>
            <ul className="mcp-import-list" aria-labelledby={addId}>
              {document.requests.map((request) => (
                <li key={request.name} className="mcp-import-name">
                  {request.name}
                </li>
              ))}
            </ul>
          </div>
        )}
        <div className="mcp-import-status" role="status">
          {document.errors.length > 0 && (
            <div className="mcp-import-group">
              <p id={skippedId} className="form-label">
                Skipped
              </p>
              <ul className="mcp-import-list mcp-import-skipped" aria-labelledby={skippedId}>
                {document.errors.map((message) => (
                  <li key={message}>{message}</li>
                ))}
              </ul>
            </div>
          )}
        </div>
        {error && (
          <div className="modal-error" role="alert">
            {error}
          </div>
        )}
        <div className="modal-actions">
          <Button variant="ghost" disabled={pending} onClick={onClose}>
            Cancel
          </Button>
          <Button disabled={count === 0} loading={pending} onClick={submit}>
            {importLabel(count)}
          </Button>
        </div>
      </div>
    </Modal>
  );
}
