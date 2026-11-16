import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@zone/ui';
import { AnalyticsFormat } from '../AnalyticsFormat';
import { AnalyticsLabel, NO_SOURCE } from '../AnalyticsLabel';
import type { SourceBreakdown } from '../types';

export const SOURCES_LABEL = 'Runs by source';

export const NO_SOURCES = 'No source has finished a run in this period.';

function busiestFirst(sources: readonly SourceBreakdown[]): SourceBreakdown[] {
  return [...sources].sort(
    (left, right) =>
      right.outcomes.total - left.outcomes.total ||
      right.merged_pull_requests - left.merged_pull_requests
  );
}

export interface SourceLeaderboardProps {
  sources: readonly SourceBreakdown[];
}

export function SourceLeaderboard({ sources }: SourceLeaderboardProps) {
  if (sources.length === 0) {
    return <p className="analytics-quiet">{NO_SOURCES}</p>;
  }
  return (
    <Table className="analytics-table" aria-label={SOURCES_LABEL}>
      <TableHeader>
        <TableRow>
          <TableHead scope="col">Source</TableHead>
          <TableHead scope="col" className="analytics-number">
            Runs
          </TableHead>
          <TableHead scope="col" className="analytics-number">
            Success
          </TableHead>
          <TableHead scope="col" className="analytics-number">
            Merged
          </TableHead>
        </TableRow>
      </TableHeader>
      <TableBody>
        {busiestFirst(sources).map((source) => (
          <TableRow key={source.source_id ?? NO_SOURCE}>
            <TableCell className="analytics-name">
              <span className="analytics-name-primary">{source.name ?? NO_SOURCE}</span>
              {source.source_type && (
                <span className="analytics-name-secondary">
                  {AnalyticsLabel.sourceType(source.source_type)}
                </span>
              )}
            </TableCell>
            <TableCell className="analytics-number">
              {AnalyticsFormat.count(source.outcomes.total)}
            </TableCell>
            <TableCell className="analytics-number">
              {AnalyticsFormat.share(
                source.outcomes.total === 0 ? null : source.outcomes.success_rate
              )}
            </TableCell>
            <TableCell className="analytics-number">
              {AnalyticsFormat.count(source.merged_pull_requests)}
            </TableCell>
          </TableRow>
        ))}
      </TableBody>
    </Table>
  );
}
