import { points, share } from '../format';
import { TrendDirectionSchema } from '../schemas';
import type { Trend, TrendDirection } from '../types';

export const NO_TREND = 'No trend yet';

const ARROWS: Record<TrendDirection, string> = {
  improving: '↑',
  steady: '→',
  declining: '↓',
};

const DIRECTIONS: Record<TrendDirection, string> = {
  improving: 'Up',
  steady: 'Steady',
  declining: 'Down',
};

export function trendText(trend: Trend): string {
  if (trend.direction === TrendDirectionSchema.enum.steady) return DIRECTIONS.steady;
  return `${DIRECTIONS[trend.direction]} ${points(trend.change)}`;
}

function comparison(trend: Trend): string {
  return `Earlier half ${share(trend.earlier_success_rate)}, later half ${share(trend.later_success_rate)}`;
}

export interface TrendIndicatorProps {
  trend: Trend;
}

export function TrendIndicator({ trend }: TrendIndicatorProps) {
  const measured = trend.earlier_success_rate !== null && trend.later_success_rate !== null;
  if (!measured) {
    return <span className="analytics-trend">{NO_TREND}</span>;
  }
  return (
    <span className="analytics-trend" data-direction={trend.direction} title={comparison(trend)}>
      <span className="analytics-trend-arrow" aria-hidden="true">
        {ARROWS[trend.direction]}
      </span>
      {trendText(trend)}
    </span>
  );
}
