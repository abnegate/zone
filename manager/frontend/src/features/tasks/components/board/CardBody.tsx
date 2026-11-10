import { Badge } from '@zone/ui';
import type { ReactNode } from 'react';
import type { StatusTint, TaskStatus } from '../../status';
import type { PrStatus, Task } from '../../types';
import { Priority } from './Priority';
import { PullRequestLink } from './PullRequestLink';
import { Stamp } from './Stamp';

export interface CardBodyProps {
  task: Task;
  projects: string;
  now: number;
  title: ReactNode;
}

const QUEUED: TaskStatus = 'queued';

const PR_TINTS: Record<PrStatus, StatusTint> = {
  pending: 'neutral',
  open: 'success',
  merged: 'accent',
  closed: 'destructive',
};

export function CardBody({ task, projects, now, title }: CardBodyProps) {
  const stamp = task.updated_at ?? task.created_at;

  return (
    <>
      <div className="task-card-heading">{title}</div>
      <div className="task-card-meta">
        {task.status === QUEUED && <Badge variant="info">Queued</Badge>}
        <span className="task-card-project">{projects}</span>
        {task.priority !== null && <Priority value={task.priority} />}
        {task.is_agentic && <Badge variant="accent">Agentic</Badge>}
        {task.pr_status && <Badge variant={PR_TINTS[task.pr_status]}>PR {task.pr_status}</Badge>}
        {task.pr_url && <PullRequestLink url={task.pr_url} />}
        {stamp && <Stamp value={stamp} now={now} />}
      </div>
    </>
  );
}
