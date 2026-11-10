import { Badge } from '@zone/ui';
import { formatDistanceToNow } from 'date-fns';
import type { ReactNode, SyntheticEvent } from 'react';
import type { StatusTint, TaskStatus } from '../../status';
import type { PrStatus, Task } from '../../types';

export interface CardBodyProps {
  task: Task;
  projects: string;
  title: ReactNode;
}

const QUEUED: TaskStatus = 'queued';

const PR_TINTS: Record<PrStatus, StatusTint> = {
  pending: 'neutral',
  open: 'success',
  merged: 'accent',
  closed: 'destructive',
};

function isolate(event: SyntheticEvent): void {
  event.stopPropagation();
}

function Stamp({ value }: { value: string }) {
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return null;
  return (
    <time className="task-card-time" dateTime={value} title={date.toLocaleString()}>
      {formatDistanceToNow(date, { addSuffix: true })}
    </time>
  );
}

function Priority({ value }: { value: number }) {
  const label = `Priority ${value}`;
  return (
    <span className="task-card-priority" role="img" aria-label={label} title={label}>
      P{value}
    </span>
  );
}

export function CardBody({ task, projects, title }: CardBodyProps) {
  const stamp = task.updated_at ?? task.created_at;

  return (
    <>
      <div className="task-card-heading">{title}</div>
      <div className="task-card-meta">
        {task.status === QUEUED && <Badge variant="info">Queued</Badge>}
        <span className="task-card-project" title={projects}>
          {projects}
        </span>
        {task.priority !== null && <Priority value={task.priority} />}
        {task.is_agentic && <Badge variant="accent">Agentic</Badge>}
        {task.pr_status && <Badge variant={PR_TINTS[task.pr_status]}>PR {task.pr_status}</Badge>}
        {task.pr_url && (
          <a
            className="task-card-pr"
            href={task.pr_url}
            target="_blank"
            rel="noopener noreferrer"
            onPointerDown={isolate}
            onClick={isolate}
            onKeyDown={isolate}
          >
            View PR
          </a>
        )}
        {stamp && <Stamp value={stamp} />}
      </div>
    </>
  );
}
