import { badgeVariants } from '@zone/ui';
import type { ChatSubagent } from '../subagentTypes';
import {
  subagentActive,
  subagentLabel,
  subagentStatusLabel,
  subagentStatusTint,
} from '../utils/subagents';
import './SubagentChip.css';
import './SubagentPulse.css';

const SHOW_HINT = 'Show in the agents box';

export interface SubagentChipProps {
  subagent: ChatSubagent;
  onShow: (id: string) => void;
}

function RobotIcon() {
  return (
    <svg
      className="chat-subagent-chip-icon"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <path d="M12 8V4H8" />
      <rect width="16" height="12" x="4" y="8" rx="2" />
      <path d="M2 14h2" />
      <path d="M20 14h2" />
      <path d="M15 13v2" />
      <path d="M9 13v2" />
    </svg>
  );
}

/** One subagent the message's turn started, which opens it in the panel's Agents box. */
export function SubagentChip({ subagent, onShow }: SubagentChipProps) {
  const label = subagentLabel(subagent);
  const status = subagentStatusLabel(subagent.status);
  const pill = badgeVariants({ variant: subagentStatusTint(subagent.status) });

  return (
    <button
      type="button"
      className="chat-subagent-chip"
      aria-label={`${label}, ${status}. ${SHOW_HINT}`}
      title={label}
      onClick={() => onShow(subagent.id)}
    >
      <RobotIcon />
      <span className="chat-subagent-chip-name">{label}</span>
      <span className={`${pill} chat-subagent-chip-status`}>
        {subagentActive(subagent) && <span className="chat-subagent-pulse" aria-hidden="true" />}
        {status}
      </span>
    </button>
  );
}
