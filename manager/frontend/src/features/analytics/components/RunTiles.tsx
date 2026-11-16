import { count, duration, measured, minutes, NOT_MEASURED, share } from '../format';
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
        value={ran ? share(outcomes.success_rate) : NOT_MEASURED}
        detail={
          ran && (
            <>
              <span>Recent {share(outcomes.recent_success_rate)}</span>
              <TrendIndicator trend={trend} />
            </>
          )
        }
      />
      <StatTile
        label="Runs"
        value={count(outcomes.total)}
        detail={`${count(outcomes.failed)} failed · ${count(outcomes.cancelled)} cancelled`}
      />
      <StatTile
        label="Median completion"
        value={measured(completion.median_seconds, completion.measured)}
        detail={timed && `Mean ${duration(completion.mean_seconds)}`}
      />
      <StatTile
        label="90th percentile"
        value={measured(completion.ninetieth_seconds, completion.measured)}
        detail={`${count(completion.measured)} timed`}
      />
      <StatTile
        label="PR merge rate"
        value={share(pullRequests.merge_rate)}
        detail={`${count(pullRequests.merged)} merged · ${count(pullRequests.closed)} closed`}
      />
      <StatTile
        label="Time to merge"
        value={minutes(pullRequests.median_minutes_to_merge)}
        detail={`${count(pullRequests.open)} still open`}
      />
    </div>
  );
}
