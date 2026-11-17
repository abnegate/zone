import { Card } from '@zone/ui';
import { Shimmer } from '../../github';

export const LOADING_LABEL = 'Loading analytics';

const TILES = ['success', 'runs', 'median', 'ninetieth', 'merge-rate', 'merge-time'] as const;

export function AnalyticsSkeleton() {
  return (
    <div className="analytics-skeleton" role="status" aria-label={LOADING_LABEL}>
      <div className="analytics-tiles">
        {TILES.map((tile) => (
          <Card key={tile} className="analytics-tile" aria-hidden="true">
            <Shimmer className="analytics-skeleton-label" />
            <Shimmer className="analytics-skeleton-value" />
          </Card>
        ))}
      </div>
      <Card className="analytics-skeleton-chart" aria-hidden="true">
        <Shimmer className="analytics-skeleton-block" />
      </Card>
    </div>
  );
}
