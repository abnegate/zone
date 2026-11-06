import { useEffect, useState } from 'react';
import type { ChatSubagents } from '../hooks/useChatSubagents';
import type { SubagentRequest } from '../subagentTypes';
import { boxTitle } from '../utils/boxes';
import { panelKey } from '../utils/panel';
import { PanelBox } from './PanelBox';
import { SubagentItem } from './SubagentItem';
import './SubagentsBox.css';

export interface SubagentsBoxProps {
  userId: string | null;
  subagents: ChatSubagents;
  shown?: SubagentRequest | null;
  onShown?: () => void;
}

export function SubagentsBox({ userId, subagents, shown = null, onShown }: SubagentsBoxProps) {
  const [reveal, setReveal] = useState(0);
  const [focus, setFocus] = useState<SubagentRequest | null>(null);

  useEffect(() => {
    if (!shown) return;
    setReveal((count) => count + 1);
    setFocus(shown);
    onShown?.();
  }, [shown, onShown]);

  return (
    <PanelBox
      title={boxTitle('subagents')}
      count={subagents.subagents.length}
      storageKey={panelKey(userId, 'subagents')}
      className="chat-subagents"
      reveal={reveal}
    >
      <ul className="chat-subagent-list">
        {subagents.subagents.map((subagent) => (
          <SubagentItem
            key={subagent.id}
            subagent={subagent}
            steps={subagents.steps[subagent.id]}
            onLoadSteps={subagents.loadSteps}
            reveal={focus?.id === subagent.id ? focus.nonce : 0}
          />
        ))}
      </ul>
      {subagents.error && (
        <p className="chat-subagents-error" role="alert">
          {subagents.error}
        </p>
      )}
    </PanelBox>
  );
}
