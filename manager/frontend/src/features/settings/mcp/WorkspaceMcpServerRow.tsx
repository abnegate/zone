import { Badge } from '@zone/ui';
import { Source, Status } from './constants';
import { OVERRIDE_LABELS, OVERRIDES, Override, overrideOf } from './override';
import { STATUS_BADGES } from './status';
import { TRANSPORT_LABELS } from './transport';
import type { WorkspaceMcpServer } from './types';

const Origin = {
  Organization: 'organization',
  Workspace: 'workspace',
  Instance: 'instance',
} as const;

type Origin = (typeof Origin)[keyof typeof Origin];

const ORIGIN_LABELS: Readonly<Record<Origin, string>> = {
  [Origin.Organization]: 'Inherited from organization',
  [Origin.Workspace]: 'Set for this workspace',
  [Origin.Instance]: 'Instance',
};

function originOf(server: WorkspaceMcpServer): Origin {
  if (server.source === Source.Instance) {
    return Origin.Instance;
  }
  return server.workspace_enabled === null ? Origin.Organization : Origin.Workspace;
}

interface WorkspaceMcpServerRowProps {
  server: WorkspaceMcpServer;
  editable: boolean;
  busy: boolean;
  onChange(override: Override): void;
}

export function WorkspaceMcpServerRow({
  server,
  editable,
  busy,
  onChange,
}: WorkspaceMcpServerRowProps) {
  const badge = STATUS_BADGES[server.status];
  const origin = originOf(server);
  const current = overrideOf(server);
  const locked = !editable || busy;

  return (
    <div className="workspace-mcp-server" role="group" aria-label={server.name}>
      <div className="workspace-mcp-server-identity">
        <span className="workspace-mcp-server-name">{server.name}</span>
        <span className="workspace-mcp-server-transport">{TRANSPORT_LABELS[server.transport]}</span>
      </div>
      <div className="workspace-mcp-server-state">
        <div role="status" className="workspace-mcp-server-status">
          <Badge variant={badge.variant}>
            {server.status === Status.Connecting && <span className="spinner" aria-hidden="true" />}
            {badge.label}
          </Badge>
        </div>
        <span
          className={`workspace-mcp-server-effective workspace-mcp-server-effective--${server.effective ? Override.On : Override.Off}`}
        >
          {server.effective ? 'On' : 'Off'}
        </span>
        <span className={`workspace-mcp-server-origin workspace-mcp-server-origin--${origin}`}>
          {ORIGIN_LABELS[origin]}
        </span>
      </div>
      {server.source === Source.Organization && server.id !== null ? (
        <div
          className="radio-group workspace-mcp-server-control"
          role="radiogroup"
          aria-label={`${server.name} in this workspace`}
        >
          {OVERRIDES.map((option) => (
            <label key={option} className="radio-option">
              <input
                type="radio"
                name={`mcp-${server.id}`}
                value={option}
                checked={current === option}
                disabled={locked || (option === Override.On && !server.enabled)}
                onChange={() => onChange(option)}
              />
              <span className="radio-label">{OVERRIDE_LABELS[option]}</span>
            </label>
          ))}
        </div>
      ) : (
        <span className="workspace-mcp-server-fixed">Set by the instance</span>
      )}
      {server.source === Source.Organization && !server.enabled && (
        <p className="workspace-mcp-server-note">Off for the organization</p>
      )}
    </div>
  );
}
