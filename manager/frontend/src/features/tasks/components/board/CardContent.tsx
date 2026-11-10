import { memo, type SyntheticEvent, useLayoutEffect, useRef } from 'react';
import { awaitsTitle } from '../../title';
import type { BoardActions, Task } from '../../types';
import { TaskTitleInput } from '../TaskTitleInput';
import { CardBody } from './CardBody';
import { CardMenu } from './CardMenu';
import { focusLost } from './focus';

export interface CardContentProps {
  task: Task;
  projects: string;
  renaming: boolean;
  actions: BoardActions;
}

function isolate(event: SyntheticEvent): void {
  event.stopPropagation();
}

export const CardContent = memo(function CardContent({
  task,
  projects,
  renaming,
  actions,
}: CardContentProps) {
  const open = useRef<HTMLButtonElement>(null);
  const wasRenaming = useRef(renaming);
  const pending = awaitsTitle(task);

  useLayoutEffect(() => {
    if (wasRenaming.current && !renaming && focusLost()) open.current?.focus();
    wasRenaming.current = renaming;
  }, [renaming]);

  const title = renaming ? (
    <TaskTitleInput
      title={task.title}
      onSave={(value) => actions.retitle(task, value)}
      onDone={() => actions.finish(task)}
    />
  ) : (
    <button
      ref={open}
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
    <>
      <CardBody task={task} projects={projects} title={title} />
      <div className="task-card-menu" onPointerDown={isolate}>
        <CardMenu task={task} actions={actions} />
      </div>
    </>
  );
});
