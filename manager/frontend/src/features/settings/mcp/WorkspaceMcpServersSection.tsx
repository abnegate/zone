import { Alert, Button, EmptyState } from '@zone/ui';
import { useId, useState } from 'react';
import { messageOf } from './failure';
import { useWorkspaceMcpServers } from './hooks';
import { keyOf } from './key';
import { type Override, overrideValue } from './override';
import { WorkspaceMcpServerRow } from './WorkspaceMcpServerRow';
import './WorkspaceMcpServersSection.css';

const UPDATE_FAILED = 'Could not update MCP servers.';

const NOTHING_PENDING: ReadonlySet<string> = new Set();

function without(pending: ReadonlySet<string>, id: string): ReadonlySet<string> {
  const next = new Set(pending);
  next.delete(id);
  return next;
}

interface WorkspaceMcpServersSectionProps {
  organizationId: string;
  workspaceId: string;
  editable: boolean;
}

export function WorkspaceMcpServersSection({
  organizationId,
  workspaceId,
  editable,
}: WorkspaceMcpServersSectionProps) {
  const headingId = useId();
  const { servers, loaded, loading, fetching, error, refresh, update } = useWorkspaceMcpServers(
    organizationId,
    workspaceId
  );
  const [pending, setPending] = useState<ReadonlySet<string>>(NOTHING_PENDING);
  const [failure, setFailure] = useState<string | null>(null);

  const change = async (id: string, override: Override): Promise<void> => {
    setFailure(null);
    setPending((current) => new Set(current).add(id));
    try {
      await update({ [id]: overrideValue(override) });
    } catch (caught) {
      setFailure(messageOf(caught, UPDATE_FAILED));
    } finally {
      setPending((current) => without(current, id));
    }
  };

  const retry = error !== null && (
    <div className="workspace-mcp-error">
      <Alert variant="destructive">{error}</Alert>
      <Button variant="ghost" size="sm" loading={fetching} onClick={() => void refresh()}>
        Retry
      </Button>
    </div>
  );

  const body = () => {
    if (loading) {
      return (
        <div className="loading-state" role="status">
          Loading MCP servers…
        </div>
      );
    }
    if (!loaded) {
      return null;
    }
    if (servers.length === 0) {
      return (
        <EmptyState
          title="No MCP servers"
          description="This organization has no MCP servers yet."
        />
      );
    }
    return (
      <div className="settings-card workspace-mcp-servers">
        {servers.map((server) => (
          <WorkspaceMcpServerRow
            key={keyOf(server)}
            server={server}
            editable={editable}
            busy={server.id !== null && pending.has(server.id)}
            onChange={(override) => {
              if (server.id !== null) {
                void change(server.id, override);
              }
            }}
          />
        ))}
      </div>
    );
  };

  return (
    <section className="settings-section" aria-labelledby={headingId}>
      <div className="section-row">
        <div className="section-row-copy">
          <h2 id={headingId} className="section-title">
            MCP servers
          </h2>
          <p className="section-description">
            Each server follows the organization unless this workspace turns it on or off.
          </p>
        </div>
      </div>

      {!editable && (
        <p className="form-hint" role="status">
          Only workspace admins can change MCP servers.
        </p>
      )}
      {failure !== null && <Alert variant="destructive">{failure}</Alert>}
      {retry}

      {body()}
    </section>
  );
}
