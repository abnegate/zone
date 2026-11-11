import { useId } from 'react';
import { stageOf, statusLabel } from '../../status';
import type { Task } from '../../types';
import { CardSensor } from './CardSensor';
import { StatusIcon } from './StatusIcon';

export interface RelatedTaskProps {
  task: Task;
  onOpen: (task: Task) => void;
}

export function RelatedTask({ task, onOpen }: RelatedTaskProps) {
  const status = useId();

  return (
    <li className="task-relation-item">
      <button
        type="button"
        className="task-relation-open"
        data-related={task.id}
        aria-label={`Open ${task.title}`}
        aria-describedby={status}
        title={task.title}
        onPointerDown={CardSensor.shield}
        onClick={() => onOpen(task)}
      >
        <StatusIcon stage={stageOf(task.status).id} />
        <span className="task-relation-title">{task.title}</span>
      </button>
      <span id={status} className="sr-only">
        {statusLabel(task.status)}
      </span>
    </li>
  );
}
