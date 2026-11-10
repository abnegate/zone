import { ActionMenu, Badge, buttonVariants, Checkbox } from '@zone/ui';
import { useId, useState } from 'react';
import { Source, Status } from './constants';
import { STATUS_BADGES } from './status';
import { toggledTools, toolSummary } from './tools';
import { TRANSPORT_LABELS } from './transport';
import type { McpServer } from './types';
import './McpServerRow.css';

interface McpServerRowProps {
  server: McpServer;
  canManage: boolean;
  busy: boolean;
  onToggle(enabled: boolean): void;
  onTools(tools: string[] | null): void;
  onEdit(): void;
  onRemove(): void;
  onReconnect(): void;
}

export function McpServerRow({
  server,
  canManage,
  busy,
  onToggle,
  onTools,
  onEdit,
  onRemove,
  onReconnect,
}: McpServerRowProps) {
  const base = useId();
  const [expanded, setExpanded] = useState(false);
  const panelId = `${base}-tools`;
  const instance = server.source === Source.Instance;
  const controls = canManage && server.source === Source.Organization;
  const locked = !canManage || busy || instance;
  const badge = STATUS_BADGES[server.status];
  const summary = toolSummary(server);

  return (
    <li className="mcp-server" aria-label={server.name}>
      <div className="mcp-server-identity">
        <span className="mcp-server-name">{server.name}</span>
        <span className="mcp-server-transport">{TRANSPORT_LABELS[server.transport]}</span>
        {instance && <Badge variant="outline">Instance</Badge>}
      </div>
      <div className="mcp-server-state">
        <div role="status" className="mcp-server-status">
          <Badge variant={badge.variant}>
            {server.status === Status.Connecting && <span className="spinner" aria-hidden="true" />}
            {badge.label}
          </Badge>
        </div>
        {summary !== null && <span className="mcp-server-summary">{summary}</span>}
      </div>
      {controls && (
        <div className="mcp-server-actions">
          <label className="toggle-label mcp-server-toggle">
            <span className="toggle-wrapper">
              <input
                type="checkbox"
                role="switch"
                aria-label={`Enable ${server.name}`}
                checked={server.enabled}
                aria-checked={server.enabled}
                disabled={busy}
                onChange={(event) => onToggle(event.target.checked)}
              />
              <span className="toggle-slider" />
            </span>
          </label>
          <ActionMenu
            label={`Actions for ${server.name}`}
            items={[
              { label: 'Edit', onSelect: onEdit },
              { label: 'Reconnect', onSelect: onReconnect, disabled: !server.enabled },
              { label: 'Remove', onSelect: onRemove, destructive: true },
            ]}
          />
        </div>
      )}
      {server.status === Status.Failed && server.error !== null && (
        <p className="mcp-server-error">{server.error}</p>
      )}
      {server.available_tools.length > 0 && (
        <div className="mcp-server-tools">
          <button
            type="button"
            className={buttonVariants({
              variant: 'ghost',
              size: 'sm',
              className: 'mcp-server-tools-toggle',
            })}
            aria-expanded={expanded}
            aria-controls={panelId}
            aria-label={`Tools for ${server.name}`}
            onClick={() => setExpanded((current) => !current)}
          >
            Tools
          </button>
          <ul id={panelId} className="mcp-server-tool-list" hidden={!expanded}>
            {server.available_tools.map((tool, index) => (
              <li key={tool}>
                <Checkbox
                  id={`${base}-tool-${index}`}
                  label={tool}
                  checked={server.tools === null || server.tools.includes(tool)}
                  disabled={locked}
                  onCheckedChange={(checked) => onTools(toggledTools(server, tool, checked))}
                />
              </li>
            ))}
          </ul>
        </div>
      )}
    </li>
  );
}
