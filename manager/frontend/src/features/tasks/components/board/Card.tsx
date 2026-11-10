import { useDraggable } from '@dnd-kit/core';
import type { SyntheticEvent } from 'react';
import { awaitsTitle } from '../../title';
import type { BoardActions, Task } from '../../types';
import { TaskTitleInput } from '../TaskTitleInput';
import { CardBody } from './CardBody';
import { CardMenu } from './CardMenu';
import './Card.css';

export interface CardProps {
  task: Task;
  projects: string;
  renaming: boolean;
  actions: BoardActions;
}

function isolate(event: SyntheticEvent): void {
  event.stopPropagation();
}

function GripIcon() {
  return (
    <svg viewBox="0 0 10 16" fill="currentColor" aria-hidden="true">
      <circle cx="3" cy="3" r="1.25" />
      <circle cx="7" cy="3" r="1.25" />
      <circle cx="3" cy="8" r="1.25" />
      <circle cx="7" cy="8" r="1.25" />
      <circle cx="3" cy="13" r="1.25" />
      <circle cx="7" cy="13" r="1.25" />
    </svg>
  );
}

export function Card({ task, projects, renaming, actions }: CardProps) {
  const { attributes, listeners, setNodeRef, setActivatorNodeRef, isDragging } = useDraggable({
    id: task.id,
    data: { task },
    disabled: renaming,
  });
  const pending = awaitsTitle(task);
  const className = [
    'task-card',
    task.is_agentic && 'task-card-agentic',
    isDragging && 'task-card-dragging',
  ]
    .filter(Boolean)
    .join(' ');

  const title = renaming ? (
    <TaskTitleInput
      title={task.title}
      onSave={(value) => actions.retitle(task, value)}
      onDone={actions.finish}
    />
  ) : (
    <button
      type="button"
      className={pending ? 'task-card-open task-card-title-pending' : 'task-card-open'}
      title={task.title}
      aria-busy={pending || undefined}
      onClick={() => actions.open(task)}
    >
      {task.title}
    </button>
  );

  return (
    <li ref={setNodeRef} className={className} data-task-id={task.id} {...listeners}>
      <button
        type="button"
        ref={setActivatorNodeRef}
        className="task-card-handle"
        {...attributes}
        aria-label={`Move ${task.title}`}
      >
        <GripIcon />
      </button>
      <CardBody task={task} projects={projects} title={title} />
      <div className="task-card-menu" onPointerDown={isolate}>
        <CardMenu task={task} actions={actions} />
      </div>
    </li>
  );
}
