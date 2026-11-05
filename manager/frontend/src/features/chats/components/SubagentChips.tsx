import type { ChatSubagent } from '../subagentTypes';
import { SubagentChip } from './SubagentChip';

export interface SubagentChipsProps {
  subagents: readonly ChatSubagent[];
  onShow: (id: string) => void;
}

export function SubagentChips({ subagents, onShow }: SubagentChipsProps) {
  if (subagents.length === 0) return null;

  return (
    <ul className="chat-subagent-chips" aria-label="Agents">
      {subagents.map((subagent) => (
        <li key={subagent.id}>
          <SubagentChip subagent={subagent} onShow={onShow} />
        </li>
      ))}
    </ul>
  );
}
