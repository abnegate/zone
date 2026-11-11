import { type ReactNode, useContext } from 'react';
import { PullRequest } from '../../PullRequest';
import { BLOCKS, DEPENDS_ON, type Relations, related } from '../../relations';
import { statusLabel, type TaskStatus, unsettledFirst } from '../../status';
import type { Task } from '../../types';
import { AgenticIcon } from './AgenticIcon';
import { Priority } from './Priority';
import { PullRequestRow } from './PullRequestRow';
import { Relation } from './Relation';
import { Stamp } from './Stamp';
import { Unfolded } from './Unfolded';

export interface CardBodyProps {
  task: Task;
  projects: string;
  relations: Relations;
  now: number;
  title: ReactNode;
  onOpen: (task: Task) => void;
}

const QUEUED: TaskStatus = 'queued';

const AGENTIC = 'Agentic';

const RELATED_LIMIT = 1;

export function CardBody({ task, projects, relations, now, title, onOpen }: CardBodyProps) {
  const stamp = task.updated_at ?? task.created_at;
  const pullRequest = PullRequest.of(task);
  const unfolded = useContext(Unfolded);
  const fold = (label: string) => {
    const key = `${task.id}:${label}`;
    return {
      defaultExpanded: unfolded?.has(key),
      onExpand: (expanded: boolean) => {
        if (expanded) unfolded?.add(key);
        else unfolded?.delete(key);
      },
    };
  };

  return (
    <>
      <div className="task-card-heading">{title}</div>
      <div className="task-card-meta">
        {task.priority !== null && <Priority value={task.priority} />}
        {projects && <span className="task-card-fact task-card-project">{projects}</span>}
        {task.status === QUEUED && (
          <span className="task-card-fact task-card-queued">{statusLabel(QUEUED)}</span>
        )}
        {task.is_agentic && (
          <span className="task-card-agentic-mark" role="img" aria-label={AGENTIC} title={AGENTIC}>
            <AgenticIcon />
          </span>
        )}
        {stamp && <Stamp value={stamp} now={now} />}
      </div>
      {(pullRequest || related(relations)) && (
        <div className="task-card-links">
          {pullRequest && <PullRequestRow pullRequest={pullRequest} />}
          <Relation
            label={DEPENDS_ON}
            tasks={unsettledFirst(relations.dependencies)}
            unknown={relations.unknown}
            limit={RELATED_LIMIT}
            onOpen={onOpen}
            {...fold(DEPENDS_ON)}
          />
          <Relation
            label={BLOCKS}
            tasks={unsettledFirst(relations.dependents)}
            limit={RELATED_LIMIT}
            onOpen={onOpen}
            {...fold(BLOCKS)}
          />
        </div>
      )}
    </>
  );
}
