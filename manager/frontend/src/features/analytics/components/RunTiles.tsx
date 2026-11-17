import { AnalyticsFormat, NOT_MEASURED } from '../AnalyticsFormat';
import type { Overview } from '../types';
import { StatTile } from './StatTile';
import { TrendIndicator } from './TrendIndicator';

export interface RunTilesProps {
  overview: Overview;
}

export function RunTiles({ overview }: RunTilesProps) {
  const { outcomes, completion, trend } = overview.runs;
  const pullRequests = overview.pull_requests;
  const ran = outcomes.total > 0;
  const timed = completion.measured > 0;
  return (
    <div className="analytics-tiles">
      <StatTile
        label="Success rate"
        value={ran ? AnalyticsFormat.share(outcomes.success_rate) : NOT_MEASURED}
        detail={
          ran && (
            <>
              <span>Recent {AnalyticsFormat.share(outcomes.recent_success_rate)}</span>
              <TrendIndicator trend={trend} />
            </>
          )
        }
      />
      <StatTile
        label="Runs"
        value={AnalyticsFormat.count(outcomes.total)}
        detail={`${AnalyticsFormat.count(outcomes.failed)} failed · ${AnalyticsFormat.count(outcomes.cancelled)} cancelled`}
      />
      <StatTile
        label="Median completion"
        value={AnalyticsFormat.measured(completion.median_seconds, completion.measured)}
        detail={timed && `Mean ${AnalyticsFormat.duration(completion.mean_seconds)}`}
      />
      <StatTile
        label="90th percentile"
        value={AnalyticsFormat.measured(completion.ninetieth_seconds, completion.measured)}
        detail={`${AnalyticsFormat.count(completion.measured)} timed`}
      />
      <StatTile
        label="PR merge rate"
        value={AnalyticsFormat.share(pullRequests.merge_rate)}
        detail={`${AnalyticsFormat.count(pullRequests.merged)} merged · ${AnalyticsFormat.count(pullRequests.closed)} closed`}
      />
      <StatTile
        label="Time to merge"
        value={AnalyticsFormat.minutes(pullRequests.median_minutes_to_merge)}
        detail={`${AnalyticsFormat.count(pullRequests.open)} still open`}
      />
    </div>
  );
}
