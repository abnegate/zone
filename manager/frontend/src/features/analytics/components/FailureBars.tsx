import { AnalyticsFormat } from '../AnalyticsFormat';
import { AnalyticsLabel } from '../AnalyticsLabel';
import { Tone } from '../Tone';
import type { CategoryStatistics } from '../types';
import { MeterRow } from './MeterRow';

export const NO_FAILURES = 'No failed runs in this period.';

export const FAILURES_LABEL = 'Failure kinds';

function detail(statistics: CategoryStatistics): string {
  const tasks = statistics.distinct_tasks === 1 ? 'task' : 'tasks';
  return `${AnalyticsFormat.count(statistics.distinct_tasks)} ${tasks} · last ${AnalyticsFormat.date(statistics.last_seen)}`;
}

export interface FailureBarsProps {
  categories: readonly CategoryStatistics[];
}

export function FailureBars({ categories }: FailureBarsProps) {
  if (categories.length === 0) {
    return <p className="analytics-quiet">{NO_FAILURES}</p>;
  }
  const maximum = Math.max(...categories.map((statistics) => statistics.occurrences));
  return (
    <ul className="analytics-meters" aria-label={FAILURES_LABEL}>
      {categories.map((statistics) => (
        <MeterRow
          key={statistics.category}
          label={AnalyticsLabel.category(statistics.category)}
          value={statistics.occurrences}
          maximum={maximum}
          display={AnalyticsFormat.count(statistics.occurrences)}
          tone={Tone.Destructive}
          detail={detail(statistics)}
        />
      ))}
    </ul>
  );
}
