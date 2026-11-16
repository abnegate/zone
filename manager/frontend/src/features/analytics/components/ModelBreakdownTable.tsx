import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@zone/ui';
import { AnalyticsFormat } from '../AnalyticsFormat';
import { NO_MODEL } from '../AnalyticsLabel';
import type { ModelBreakdown } from '../types';

export const MODELS_LABEL = 'Runs by model';

export const NO_MODELS = 'No model has finished a run in this period.';

export interface ModelBreakdownTableProps {
  models: readonly ModelBreakdown[];
}

export function ModelBreakdownTable({ models }: ModelBreakdownTableProps) {
  if (models.length === 0) {
    return <p className="analytics-quiet">{NO_MODELS}</p>;
  }
  const ordered = [...models].sort((left, right) => right.outcomes.total - left.outcomes.total);
  return (
    <Table className="analytics-table" aria-label={MODELS_LABEL}>
      <TableHeader>
        <TableRow>
          <TableHead scope="col">Model</TableHead>
          <TableHead scope="col" className="analytics-number">
            Runs
          </TableHead>
          <TableHead scope="col" className="analytics-number">
            Success
          </TableHead>
          <TableHead scope="col" className="analytics-number">
            Median
          </TableHead>
          <TableHead scope="col" className="analytics-number">
            90th
          </TableHead>
        </TableRow>
      </TableHeader>
      <TableBody>
        {ordered.map(({ model, outcomes, completion }) => (
          <TableRow key={model ?? NO_MODEL}>
            <TableCell className="analytics-name">
              <span className="analytics-name-primary">{model ?? NO_MODEL}</span>
            </TableCell>
            <TableCell className="analytics-number">
              {AnalyticsFormat.count(outcomes.total)}
            </TableCell>
            <TableCell className="analytics-number">
              {AnalyticsFormat.share(outcomes.total === 0 ? null : outcomes.success_rate)}
            </TableCell>
            <TableCell className="analytics-number">
              {AnalyticsFormat.measured(completion.median_seconds, completion.measured)}
            </TableCell>
            <TableCell className="analytics-number">
              {AnalyticsFormat.measured(completion.ninetieth_seconds, completion.measured)}
            </TableCell>
          </TableRow>
        ))}
      </TableBody>
    </Table>
  );
}
