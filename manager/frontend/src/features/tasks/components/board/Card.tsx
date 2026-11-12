import { useDraggable } from '@dnd-kit/core';
import { Checkbox } from '@zone/ui';
import { gestureOf, keepText, TOGGLE } from '../../gesture';
import type { Relations } from '../../relations';
import type { TitleDraft } from '../../TitleDraft';
import type { BoardActions, Task } from '../../types';
import { CardContent } from './CardContent';
import { CardSensor } from './CardSensor';
import { GripIcon } from './GripIcon';
import './Card.css';

export interface CardProps {
  task: Task;
  projects: string;
  relations: Relations;
  now: number;
  renaming: TitleDraft | null;
  selected: boolean;
  actions: BoardActions;
}

export function Card({ task, projects, relations, now, renaming, selected, actions }: CardProps) {
  const { attributes, listeners, setNodeRef, setActivatorNodeRef, isDragging } = useDraggable({
    id: task.id,
    data: { task },
    disabled: renaming !== null,
  });
  const className = isDragging ? 'task-card task-card-dragging' : 'task-card';

  return (
    <li
      ref={setNodeRef}
      className={className}
      data-task-id={task.id}
      data-selected={selected || undefined}
      {...listeners}
    >
      <button
        type="button"
        ref={setActivatorNodeRef}
        className="task-card-handle"
        {...attributes}
        aria-label={`Move ${task.title}`}
      >
        <GripIcon />
      </button>
      <div className="task-card-selector" onPointerDown={CardSensor.shield}>
        <Checkbox
          className="task-card-select"
          aria-label={`Select ${task.title}`}
          checked={selected}
          onMouseDown={keepText}
          onClick={(event) => actions.select(task, gestureOf(event) ?? TOGGLE)}
        />
      </div>
      <CardContent
        task={task}
        projects={projects}
        relations={relations}
        now={now}
        renaming={renaming}
        selected={selected}
        actions={actions}
      />
    </li>
  );
}
