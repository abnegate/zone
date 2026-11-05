export interface AgentPermissionProps {
  name: string;
  hint: string;
  title: string;
  enabled: boolean;
  warning?: boolean;
  testId: string;
  onToggle: () => void;
}

export function AgentPermission({
  name,
  hint,
  title,
  enabled,
  warning = false,
  testId,
  onToggle,
}: AgentPermissionProps) {
  return (
    <button
      type="button"
      className={warning ? 'agent-permission agent-permission--warning' : 'agent-permission'}
      onClick={onToggle}
      aria-pressed={enabled}
      aria-label={name}
      title={title}
      data-testid={testId}
    >
      <span className="agent-permission-text">
        <span className="agent-permission-name">{name}</span>
        <span className="agent-permission-hint">{hint}</span>
      </span>
      <span className="agent-permission-state" aria-hidden="true">
        {enabled ? 'On' : 'Off'}
      </span>
    </button>
  );
}
