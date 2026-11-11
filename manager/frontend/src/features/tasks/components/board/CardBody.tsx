import { Badge } from '@zone/ui';
import type { ReactNode } from 'react';
import { PullRequest } from '../../PullRequest';
import { BLOCKS, DEPENDS_ON, type Relations } from '../../relations';
import type { TaskStatus } from '../../status';
import type { Task } from '../../types';
import { Priority } from './Priority';
import { PullRequestLink } from './PullRequestLink';
import { Relation } from './Relation';
import { Stamp } from './Stamp';

export interface CardBodyProps {
  task: Task;
  projects: string;
  relations: Relations;
  now: number;
  title: ReactNode;
  onOpen: (task: Task) => void;
}

const QUEUED: TaskStatus = 'queued';

const RELATED_LIMIT = 3;

export function CardBody({ task, projects, relations, now, title, onOpen }: CardBodyProps) {
  const stamp = task.updated_at ?? task.created_at;
  const pullRequest = PullRequest.of(task);

  return (
    <>
      <div className="task-card-heading">{title}</div>
      <div className="task-card-meta">
        {task.status === QUEUED && <Badge variant="info">Queued</Badge>}
        {projects && <span className="task-card-project">{projects}</span>}
        {task.priority !== null && <Priority value={task.priority} />}
        {task.is_agentic && <Badge variant="accent">Agentic</Badge>}
        {pullRequest && <PullRequestLink pullRequest={pullRequest} />}
        {stamp && <Stamp value={stamp} now={now} />}
      </div>
      <Relation
        label={DEPENDS_ON}
        tasks={relations.dependencies}
        unknown={relations.unknown}
        limit={RELATED_LIMIT}
        onOpen={onOpen}
      />
      <Relation label={BLOCKS} tasks={relations.dependents} limit={RELATED_LIMIT} onOpen={onOpen} />
    </>
  );
}
