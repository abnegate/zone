import { useId, useState } from 'react';
import type { Task } from '../../types';
import { CardSensor } from './CardSensor';
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

const MORE = 'more';

const LESS = 'Show less';

export function Relation({
  label,
  tasks,
  unknown = 0,
  limit = tasks.length,
  onOpen,
}: RelationProps) {
  const heading = useId();
  const [expanded, setExpanded] = useState(false);
  if (tasks.length === 0 && unknown === 0) return null;
  const folds = tasks.length > limit;
  const shown = folds && !expanded ? tasks.slice(0, limit) : tasks;
  const hidden = tasks.length - shown.length;

  return (
    <div className="task-relation" data-expanded={(folds && expanded) || undefined}>
      <span id={heading} className="task-relation-label">
        {label}
      </span>
      <ul className="task-relation-list" aria-labelledby={heading}>
        {shown.map((task) => (
          <RelatedTask key={task.id} task={task} onOpen={onOpen} />
        ))}
        {hidden === 0 && unknown > 0 && (
          <li className="task-relation-rest">
            {unknown} {UNKNOWN}
          </li>
        )}
        {folds && (
          <li className="task-relation-fold">
            <button
              type="button"
              className="task-relation-toggle"
              data-related={label}
              aria-expanded={expanded}
              onPointerDown={CardSensor.shield}
              onClick={() => setExpanded((open) => !open)}
            >
              {expanded ? (
                LESS
              ) : (
                <>
                  +{hidden + unknown}
                  <span className="sr-only"> {MORE}</span>
                </>
              )}
            </button>
          </li>
        )}
      </ul>
    </div>
  );
}
