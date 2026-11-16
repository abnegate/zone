import { AnalyticsFormat } from '../AnalyticsFormat';
import type { Overview } from '../types';

export const MAXIMUM_RUNS = 10_000;

export const MAXIMUM_PULL_REQUESTS = 10_000;

export function runsNotice(measuredFrom: string | null): string {
  const newest = `Showing the newest ${AnalyticsFormat.count(MAXIMUM_RUNS)} runs`;
  return measuredFrom === null
    ? `${newest} in this period.`
    : `${newest}, since ${AnalyticsFormat.date(measuredFrom)}.`;
}

export const PULL_REQUESTS_NOTICE = `Pull request figures cover the newest ${AnalyticsFormat.count(MAXIMUM_PULL_REQUESTS)} pull requests in this period.`;

export function truncationNotices({
  truncated,
  measured_from,
}: Pick<Overview, 'truncated' | 'measured_from'>): string[] {
  return [
    ...(truncated.runs ? [runsNotice(measured_from)] : []),
    ...(truncated.pull_requests ? [PULL_REQUESTS_NOTICE] : []),
  ];
}

export interface TruncationNoticeProps {
  overview: Pick<Overview, 'truncated' | 'measured_from'>;
}

export function TruncationNotice({ overview }: TruncationNoticeProps) {
  const notices = truncationNotices(overview);
  if (notices.length === 0) return null;
  return (
    <p className="analytics-notice" role="status">
      {notices.join(' ')}
    </p>
  );
}
