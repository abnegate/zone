import { Button, EmptyState } from '@zone/ui';
import { type ComponentProps, useEffect, useId, useRef, useState } from 'react';
import { Source } from './constants';
import { messageOf } from './failure';
import { type McpImportResult, useMcpServers } from './hooks';
import { keyOf } from './key';
import { McpImportDialog } from './McpImportDialog';
import { McpRemoveDialog } from './McpRemoveDialog';
import { McpServerForm } from './McpServerForm';
import { McpServerRow } from './McpServerRow';
import { type McpServerSubmission, SubmissionKind } from './submission';
import { COMMANDS_OFF_HINT } from './transport';
import type { CreateMcpServerRequest, McpServer, UpdateMcpServerRequest } from './types';
import './McpServersSection.css';

export const READ_ONLY_HINT = 'Only organization admins can change MCP servers.';
export const CHANGE_FAILED = 'Failed to update the MCP server';

type RowActions = Pick<
  ComponentProps<typeof McpServerRow>,
  'onToggle' | 'onTools' | 'onEdit' | 'onRemove' | 'onReconnect'
>;

interface ManagedServer {
  id: string;
  server: McpServer;
}

const IDLE: RowActions = {
  onToggle: () => {},
  onTools: () => {},
  onEdit: () => {},
  onRemove: () => {},
  onReconnect: () => {},
};

function managed(server: McpServer): ManagedServer | null {
  return server.source === Source.Organization && server.id !== null
    ? { id: server.id, server }
    : null;
}

export function importedNotice(count: number): string {
  return count === 1 ? 'Imported 1 server.' : `Imported ${count} servers.`;
}

interface McpServersSectionProps {
  organizationId: string;
  canManage: boolean;
}

export function McpServersSection({ organizationId, canManage }: McpServersSectionProps) {
  const headingId = useId();
  const hintId = useId();
  const {
    servers,
    commands,
    loaded,
    loading,
    fetching,
    error,
    refresh,
    create,
    update,
    remove,
    reconnect,
    importAll,
  } = useMcpServers(organizationId);
  const [adding, setAdding] = useState(false);
  const [editing, setEditing] = useState<ManagedServer | null>(null);
  const [removing, setRemoving] = useState<ManagedServer | null>(null);
  const [importing, setImporting] = useState(false);
  const [busy, setBusy] = useState<ReadonlySet<string>>(() => new Set());
  const [failure, setFailure] = useState<string | null>(null);
  const [imported, setImported] = useState<McpImportResult | null>(null);
  const headingRef = useRef<HTMLHeadingElement>(null);
  const opener = useRef<HTMLElement | null>(null);
  const restoreFocus = useRef(false);

  useEffect(() => {
    if (!restoreFocus.current) {
      return;
    }
    restoreFocus.current = false;
    const target = opener.current?.isConnected ? opener.current : headingRef.current;
    opener.current = null;
    target?.focus();
  });

  const open = (show: () => void) => {
    opener.current = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    show();
  };

  const close = (hide: () => void) => {
    restoreFocus.current = true;
    hide();
  };

  const change = async (id: string, action: () => Promise<unknown>) => {
    setBusy((current) => new Set(current).add(id));
    setFailure(null);
    try {
      await action();
    } catch (changeFailure) {
      setFailure(messageOf(changeFailure, CHANGE_FAILED));
    } finally {
      setBusy((current) => {
        const next = new Set(current);
        next.delete(id);
        return next;
      });
    }
  };

  const actionsFor = (server: McpServer): RowActions => {
    const target = managed(server);
    if (target === null) {
      return IDLE;
    }
    const { id } = target;
    const save = (request: UpdateMcpServerRequest) => void change(id, () => update(id, request));
    return {
      onToggle: (enabled) => save({ enabled }),
      onTools: (tools) => save({ tools }),
      onEdit: () => open(() => setEditing(target)),
      onRemove: () => open(() => setRemoving(target)),
      onReconnect: () => void change(id, () => reconnect(id)),
    };
  };

  const closeForm = () =>
    close(() => {
      setAdding(false);
      setEditing(null);
    });

  const submit = async (submission: McpServerSubmission) => {
    if (submission.kind === SubmissionKind.Create) {
      await create(submission.request);
    } else if (editing !== null) {
      await update(editing.id, submission.request);
    }
    closeForm();
  };

  const confirmRemove = async (target: ManagedServer) => {
    await remove(target.id);
    opener.current = null;
    close(() => setRemoving(null));
  };

  const runImport = async (requests: CreateMcpServerRequest[]) => {
    const result = await importAll(requests);
    setImported(result);
    close(() => setImporting(false));
  };

  const openImport = () =>
    open(() => {
      setImported(null);
      setImporting(true);
    });

  return (
    <section className="mcp-servers" aria-labelledby={headingId}>
      <div className="section-row">
        <div className="section-row-copy">
          <h2 id={headingId} ref={headingRef} className="section-title" tabIndex={-1}>
            MCP servers
          </h2>
          <p className="section-description">
            Servers whose tools reach every chat and task in this organization.
          </p>
        </div>
        {canManage && loaded && (
          <div className="section-row-actions">
            <Button variant="secondary" size="sm" onClick={openImport}>
              Import JSON
            </Button>
            <Button size="sm" onClick={() => open(() => setAdding(true))}>
              Add server
            </Button>
          </div>
        )}
      </div>
      {loaded && !commands && (
        <p id={hintId} className="form-hint" role="status">
          {COMMANDS_OFF_HINT}
        </p>
      )}
      {!canManage && (
        <p className="form-hint" role="status">
          {READ_ONLY_HINT}
        </p>
      )}
      {imported !== null && imported.created.length > 0 && (
        <div className="alert alert-success" role="status">
          {importedNotice(imported.created.length)}
        </div>
      )}
      {imported !== null && imported.failures.length > 0 && (
        <div className="alert alert-error" role="alert">
          <ul className="mcp-servers-failures" aria-label="Not imported">
            {imported.failures.map(({ name, message }) => (
              <li key={name}>{`${name}: ${message}`}</li>
            ))}
          </ul>
        </div>
      )}
      {failure && (
        <div className="alert alert-error" role="alert">
          {failure}
        </div>
      )}
      {loading && (
        <div className="loading-state" role="status">
          Loading MCP servers…
        </div>
      )}
      {error !== null && (
        <div className="alert alert-error alert-inline">
          <span role="alert">{error}</span>
          <Button variant="ghost" size="sm" loading={fetching} onClick={() => void refresh()}>
            Retry
          </Button>
        </div>
      )}
      {loaded && servers.length === 0 && (
        <EmptyState
          title="No MCP servers"
          description="Add a server or import an mcpServers JSON document."
        />
      )}
      {loaded && servers.length > 0 && (
        <ul className="mcp-servers-list" aria-label="MCP servers">
          {servers.map((server) => {
            const target = managed(server);
            return (
              <McpServerRow
                key={keyOf(server)}
                server={server}
                canManage={canManage && target !== null}
                commands={commands}
                hintId={hintId}
                busy={target !== null && busy.has(target.id)}
                {...actionsFor(server)}
              />
            );
          })}
        </ul>
      )}
      {(adding || editing !== null) && (
        <McpServerForm
          key={editing?.id ?? 'new'}
          server={editing?.server}
          commands={commands}
          onSubmit={submit}
          onClose={closeForm}
        />
      )}
      {removing !== null && (
        <McpRemoveDialog
          key={removing.id}
          name={removing.server.name}
          onConfirm={() => confirmRemove(removing)}
          onClose={() => close(() => setRemoving(null))}
        />
      )}
      {importing && (
        <McpImportDialog
          commands={commands}
          onImport={runImport}
          onClose={() => close(() => setImporting(false))}
        />
      )}
    </section>
  );
}
