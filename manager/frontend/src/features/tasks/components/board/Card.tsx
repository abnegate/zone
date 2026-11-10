import { useDraggable } from '@dnd-kit/core';
import type { TitleDraft } from '../../TitleDraft';
import type { BoardActions, Task } from '../../types';
import { CardContent } from './CardContent';
import { GripIcon } from './GripIcon';
import './Card.css';

export interface CardProps {
  task: Task;
  projects: string;
  renaming: TitleDraft | null;
  actions: BoardActions;
}

export function Card({ task, projects, renaming, actions }: CardProps) {
  const { attributes, listeners, setNodeRef, setActivatorNodeRef, isDragging } = useDraggable({
    id: task.id,
    data: { task },
    disabled: renaming !== null,
  });
  const className = [
    'task-card',
    task.is_agentic && 'task-card-agentic',
    isDragging && 'task-card-dragging',
  ]
    .filter(Boolean)
    .join(' ');

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
      <CardContent task={task} projects={projects} renaming={renaming} actions={actions} />
    </li>
  );
}
