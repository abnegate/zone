import { useEffect, useId, useState } from 'react';
import type { Task } from '../../types';
import { CardSensor } from './CardSensor';
import { RelatedTask } from './RelatedTask';
import './Relation.css';

export interface RelationProps {
  label: string;
  tasks: readonly Task[];
  unknown?: number;
  limit?: number;
  defaultExpanded?: boolean;
  onExpand?: (expanded: boolean) => void;
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
  defaultExpanded = false,
  onExpand,
  onOpen,
}: RelationProps) {
  const heading = useId();
  const [expanded, setExpanded] = useState(defaultExpanded);
  const folds = tasks.length > limit;

  useEffect(() => {
    if (folds || !expanded) return;
    setExpanded(false);
    onExpand?.(false);
  }, [folds, expanded, onExpand]);

  if (tasks.length === 0 && unknown === 0) return null;
  const shown = folds && !expanded ? tasks.slice(0, limit) : tasks;
  const hidden = tasks.length - shown.length;
  const toggle = () => {
    setExpanded(!expanded);
    onExpand?.(!expanded);
  };

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
              aria-describedby={heading}
              onPointerDown={CardSensor.shield}
              onClick={toggle}
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
