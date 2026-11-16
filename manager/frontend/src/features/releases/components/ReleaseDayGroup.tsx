import { Badge } from '@zone/ui';
import { useId } from 'react';
import type { ReleaseDay } from '../utils/grouping';
import { ReleaseCard } from './ReleaseCard';

export function releaseCount(count: number): string {
  return `${count} ${count === 1 ? 'release' : 'releases'}`;
}

interface ReleaseDayGroupProps {
  day: ReleaseDay;
}

export function ReleaseDayGroup({ day }: ReleaseDayGroupProps) {
  const headingId = useId();
  const count = day.releases.length;

  return (
    <section className="release-day" aria-labelledby={headingId}>
      <h2 id={headingId} className="release-day-header">
        <span className="release-day-label">{day.label}</span>
        <Badge variant="neutral" className="release-day-count">
          <span aria-hidden="true">{count}</span>
          <span className="sr-only">{releaseCount(count)}</span>
        </Badge>
      </h2>
      <ul className="release-day-list">
        {day.releases.map((release) => (
          <li key={`${release.source_id}:${release.id}`}>
            <ReleaseCard release={release} />
          </li>
        ))}
      </ul>
    </section>
  );
}
