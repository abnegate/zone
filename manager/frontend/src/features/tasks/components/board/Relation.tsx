import { useId } from 'react';
import type { Task } from '../../types';
import { RelatedTask } from './RelatedTask';
import './Relation.css';

export interface RelationProps {
  label: string;
  tasks: readonly Task[];
  unknown?: number;
  limit?: number;
  onOpen: (task: Task) => void;
}

const UNKNOWN = 'unknown';

function remainder(hidden: number, unknown: number): string {
  const parts: string[] = [];
  if (hidden > 0) parts.push(`+${hidden} more`);
  if (unknown > 0) parts.push(`${unknown} ${UNKNOWN}`);
  return parts.join(', ');
}

export function Relation({
  label,
  tasks,
  unknown = 0,
  limit = tasks.length,
  onOpen,
}: RelationProps) {
  const heading = useId();
  if (tasks.length === 0 && unknown === 0) return null;
  const shown = tasks.slice(0, limit);
  const rest = remainder(tasks.length - shown.length, unknown);

  return (
    <div className="task-relation">
      <span id={heading} className="task-relation-label">
        {label}
      </span>
      <ul className="task-relation-list" aria-labelledby={heading}>
        {shown.map((task) => (
          <RelatedTask key={task.id} task={task} onOpen={onOpen} />
        ))}
        {rest && <li className="task-relation-rest">{rest}</li>}
      </ul>
    </div>
  );
}
