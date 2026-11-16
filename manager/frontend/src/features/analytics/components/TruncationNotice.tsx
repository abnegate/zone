import { AnalyticsFormat } from '../AnalyticsFormat';
import type { Overview } from '../types';

export const MAXIMUM_RUNS = 10_000;

export const MAXIMUM_PULL_REQUESTS = 10_000;

export function runsNotice(measuredFrom: string | null, windowEnd: string): string {
  const newest = `Showing the newest ${AnalyticsFormat.count(MAXIMUM_RUNS)} runs`;
  if (measuredFrom === null) return `${newest} in this period.`;
  if (measuredFrom === windowEnd) return `${newest}, all too recent to chart yet.`;
  return `${newest}, since ${AnalyticsFormat.date(measuredFrom)}.`;
}

export const PULL_REQUESTS_NOTICE = `Pull request figures cover the newest ${AnalyticsFormat.count(MAXIMUM_PULL_REQUESTS)} pull requests in this period.`;

export function truncationNotices({
  truncated,
  measured_from,
  window,
}: Pick<Overview, 'truncated' | 'measured_from' | 'window'>): string[] {
  return [
    ...(truncated.runs ? [runsNotice(measured_from, window.end)] : []),
    ...(truncated.pull_requests ? [PULL_REQUESTS_NOTICE] : []),
  ];
}

export interface TruncationNoticeProps {
  overview: Pick<Overview, 'truncated' | 'measured_from' | 'window'>;
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
