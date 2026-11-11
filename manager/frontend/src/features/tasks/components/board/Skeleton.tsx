import { STAGES } from '../../status';
import { StatusIcon } from './StatusIcon';
import './Board.css';
import './Column.css';

const PLACEHOLDERS = ['first', 'second'] as const;

const LOADING = 'Loading tasks';

export function Skeleton() {
  return (
    <>
      <p className="sr-only" role="status">
        {LOADING}
      </p>
      <div className="board" aria-hidden="true">
        {STAGES.map((stage) => (
          <div key={stage.id} className="board-column" data-stage={stage.id}>
            <div className="board-column-header">
              <StatusIcon stage={stage.id} />
              <h2>{stage.label}</h2>
              <span className="board-skeleton board-skeleton-count" />
            </div>
            <div className="board-column-body">
              <div className="board-column-cards">
                {PLACEHOLDERS.map((placeholder) => (
                  <div key={placeholder} className="task-card board-skeleton-card">
                    <div className="board-skeleton board-skeleton-title" />
                    <div className="board-skeleton board-skeleton-text" />
                    <div className="board-skeleton-meta">
                      <div className="board-skeleton board-skeleton-badge" />
                      <div className="board-skeleton board-skeleton-tag" />
                    </div>
                  </div>
                ))}
              </div>
            </div>
          </div>
        ))}
      </div>
    </>
  );
}
