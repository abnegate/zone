import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@zone/ui';
import { SectionCard } from '../../../analytics/components/SectionCard';
import {
  NAME_COLUMN,
  NOTHING_IN_RANGE,
  REQUESTS_COLUMN,
  SPEND_COLUMN,
  TOKENS_COLUMN,
} from '../labels';
import { count, exactMoney, money } from '../money';
import type { Usage } from '../schemas';

export interface BreakdownRow {
  key: string;
  name: string;
  detail?: string;
  muted?: boolean;
  usage: Usage;
}

interface SpendBreakdownProps {
  title: string;
  rows: readonly BreakdownRow[];
}

export function SpendBreakdown({ title, rows }: SpendBreakdownProps) {
  return (
    <SectionCard title={title} className="spend-breakdown">
      {rows.length === 0 ? (
        <p className="analytics-quiet">{NOTHING_IN_RANGE}</p>
      ) : (
        <Table className="analytics-table spend-table" aria-label={title}>
          <TableHeader>
            <TableRow>
              <TableHead scope="col">{NAME_COLUMN}</TableHead>
              <TableHead scope="col" className="analytics-number">
                {SPEND_COLUMN}
              </TableHead>
              <TableHead scope="col" className="analytics-number">
                {REQUESTS_COLUMN}
              </TableHead>
              <TableHead scope="col" className="analytics-number spend-tokens">
                {TOKENS_COLUMN}
              </TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {rows.map(({ key, name, detail, muted, usage }) => (
              <TableRow key={key}>
                <TableCell className="analytics-name">
                  <span
                    className={
                      muted ? 'analytics-name-primary spend-muted' : 'analytics-name-primary'
                    }
                  >
                    {name}
                  </span>
                  {detail && <span className="analytics-name-secondary">{detail}</span>}
                </TableCell>
                <TableCell className="analytics-number">
                  <span title={exactMoney(usage.spend_microdollars)}>
                    {money(usage.spend_microdollars)}
                  </span>
                </TableCell>
                <TableCell className="analytics-number">{count(usage.requests)}</TableCell>
                <TableCell className="analytics-number spend-tokens">
                  {count(usage.prompt_tokens + usage.completion_tokens)}
                </TableCell>
              </TableRow>
            ))}
          </TableBody>
        </Table>
      )}
    </SectionCard>
  );
}
