import { Badge } from '@zone/ui';
import { Link } from 'react-router-dom';
import { statusLabel, statusTint } from '../../tasks/status';
import type { ChatTask } from '../types';
import { publicationName, runProgress, runReading } from '../utils/tasks';

export interface TaskItemProps {
  task: ChatTask;
}

export function TaskItem({ task }: TaskItemProps) {
  const reading = runReading(task.run);
  const progress = runProgress(task.run);
  const publication = task.pull_request;

  return (
    <li className="chat-task" data-task={task.id}>
      <div className="chat-task-heading">
        <Link className="chat-task-title" to={task.href} title={task.title}>
          {task.title}
        </Link>
        <Badge className="chat-task-status" variant={statusTint(task.status)}>
          {statusLabel(task.status)}
        </Badge>
      </div>
      {(reading || publication) && (
        <div className="chat-task-meta">
          {reading && <span className="chat-task-run">{reading}</span>}
          {progress !== null && <span className="chat-task-percent">{progress}%</span>}
          {publication && (
            <a
              className="chat-task-pull-request"
              href={publication.url}
              target="_blank"
              rel="noopener noreferrer"
            >
              {publicationName(publication.url)}
            </a>
          )}
        </div>
      )}
      {progress !== null && (
        <div
          className="chat-task-progress"
          role="progressbar"
          aria-label={`${task.title} progress`}
          aria-valuemin={0}
          aria-valuemax={100}
          aria-valuenow={progress}
        >
          <span className="chat-task-progress-fill" style={{ width: `${progress}%` }} />
        </div>
      )}
    </li>
  );
}
