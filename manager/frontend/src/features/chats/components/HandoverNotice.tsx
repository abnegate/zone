import type { ReactElement } from 'react';
import type { Handover } from '../types';
import { handoverCause, handoverTarget } from '../utils/handover';

interface HandoverNoticeProps {
  handover: Handover;
  agentChanged: boolean;
  now?: number;
}

export function HandoverNotice({
  handover,
  agentChanged,
  now = Date.now(),
}: HandoverNoticeProps): ReactElement {
  return (
    <div className="handover-notice" role="note" data-testid="handover-notice">
      <span className="handover-notice-text">
        Switched to <strong>{handoverTarget(handover, agentChanged)}</strong> —{' '}
        {handoverCause(handover, now)}
      </span>
    </div>
  );
}
