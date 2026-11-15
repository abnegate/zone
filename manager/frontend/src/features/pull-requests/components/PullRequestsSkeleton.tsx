import { Shimmer } from '../../github';

const LOADING = 'Loading pull requests';

const GROUPS = [
  { name: 'first', cards: ['first', 'second'] },
  { name: 'second', cards: ['first', 'second', 'third'] },
] as const;

export function PullRequestsSkeleton() {
  return (
    <div className="pull-requests-skeleton">
      <p className="sr-only" role="status">
        {LOADING}
      </p>
      <div className="pull-requests-skeleton-pane" aria-hidden="true">
        {GROUPS.map((group) => (
          <div key={group.name} className="pull-request-group">
            <div className="pull-request-group-header">
              <Shimmer className="pull-requests-shimmer-label" />
            </div>
            {group.cards.map((card) => (
              <div key={card} className="card--list pull-request-card pull-requests-skeleton-card">
                <Shimmer className="pull-requests-shimmer-title" />
                <Shimmer className="pull-requests-shimmer-meta" />
              </div>
            ))}
          </div>
        ))}
      </div>
    </div>
  );
}
