import { memo, useLayoutEffect, useRef } from 'react';
import type { TitleDraft } from '../../TitleDraft';
import { awaitsTitle } from '../../title';
import type { BoardActions, Task } from '../../types';
import { TaskTitleInput } from '../TaskTitleInput';
import { CardBody } from './CardBody';
import { CardMenu } from './CardMenu';
import { CardSensor } from './CardSensor';

export interface CardContentProps {
  task: Task;
  projects: string;
  now: number;
  renaming: TitleDraft | null;
  actions: BoardActions;
}

export const CardContent = memo(function CardContent({
  task,
  projects,
  now,
  renaming,
  actions,
}: CardContentProps) {
  const open = useRef<HTMLButtonElement>(null);
  const wasRenaming = useRef(renaming !== null);
  const pending = awaitsTitle(task);

  useLayoutEffect(() => {
    const lost = document.activeElement === null || document.activeElement === document.body;
    if (wasRenaming.current && !renaming && lost) open.current?.focus();
    wasRenaming.current = renaming !== null;
  }, [renaming]);

  const title = renaming ? (
    <TaskTitleInput
      title={task.title}
      draft={renaming}
      onSave={(value) => actions.retitle(task, value)}
      onDone={() => actions.finish(task)}
    />
  ) : (
    <button
      ref={open}
      type="button"
      className={
        pending
          ? 'task-card-title task-card-open task-card-title-pending'
          : 'task-card-title task-card-open'
      }
      title={task.title}
      aria-busy={pending || undefined}
      onClick={() => actions.open(task)}
    >
      {task.title}
    </button>
  );

  return (
    <>
      <CardBody task={task} projects={projects} now={now} title={title} />
      <div className="task-card-menu" onPointerDown={CardSensor.shield}>
        <CardMenu task={task} actions={actions} />
      </div>
    </>
  );
});
