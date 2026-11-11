import { useId } from 'react';
import { PullRequest } from '../PullRequest';
import { BLOCKS, DEPENDS_ON, type Relations, related } from '../relations';
import type { Task } from '../types';
import { PullRequestLink } from './board/PullRequestLink';
import { Relation } from './board/Relation';
import './Links.css';

export interface LinksProps {
  task: Task;
  relations: Relations;
  onOpen: (task: Task) => void;
}

export function Links({ task, relations, onOpen }: LinksProps) {
  const heading = useId();
  const pullRequest = PullRequest.of(task);
  const branch = task.branch_name;
  if (!pullRequest && !branch && !related(relations)) return null;

  return (
    <section className="form-group task-links" aria-labelledby={heading}>
      <h3 id={heading} className="form-label">
        Links
      </h3>
      <div className="task-links-body">
        {(pullRequest || branch) && (
          <div className="task-links-source">
            {pullRequest && <PullRequestLink pullRequest={pullRequest} />}
            {branch && (
              <span className="task-links-branch">
                Branch <code>{branch}</code>
              </span>
            )}
          </div>
        )}
        <Relation
          label={DEPENDS_ON}
          tasks={relations.dependencies}
          unknown={relations.unknown}
          onOpen={onOpen}
        />
        <Relation label={BLOCKS} tasks={relations.dependents} onOpen={onOpen} />
      </div>
    </section>
  );
}
