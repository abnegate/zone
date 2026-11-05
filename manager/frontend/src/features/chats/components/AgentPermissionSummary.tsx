import type { ReactElement } from 'react';

const CHIPS = {
  neutral: {
    label: 'Permissions',
    icon: 'M12 3 5 6v5c0 4.4 3 8.4 7 10 4-1.6 7-5.6 7-10V6zM9 12l2 2 4-4',
  },
  auto: {
    label: 'Auto-approve',
    icon: 'M13 2 4 14h7l-1 8 9-12h-7z',
  },
  host: {
    label: 'Host tools',
    icon: 'M10.3 3.9 1.8 18a2 2 0 0 0 1.7 3h17a2 2 0 0 0 1.7-3L13.7 3.9a2 2 0 0 0-3.4 0zM12 9v4M12 17h.01',
  },
} as const;

type Kind = keyof typeof CHIPS;

export interface AgentPermissionSummaryProps {
  autoApproved: boolean;
  hostTools: boolean;
}

export function AgentPermissionSummary({
  autoApproved,
  hostTools,
}: AgentPermissionSummaryProps): ReactElement {
  const kinds: Kind[] = [
    autoApproved ? 'auto' : 'neutral',
    ...(hostTools ? (['host'] as const) : []),
  ];
  return (
    <span className="agent-permission-summary">
      {kinds.map((kind) => (
        <span key={kind} className={`agent-permission-chip agent-permission-chip--${kind}`}>
          <svg
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            strokeWidth="2"
            strokeLinecap="round"
            strokeLinejoin="round"
            aria-hidden="true"
          >
            <path d={CHIPS[kind].icon} />
          </svg>
          <span className="agent-permission-chip-label">{CHIPS[kind].label}</span>
        </span>
      ))}
    </span>
  );
}
