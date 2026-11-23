import {
  Badge,
  Button,
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@zone/ui';
import { useId } from 'react';
import { absoluteTime, relativeTime } from '../../../../shared/utils/time';
import { ToneBadge } from '../../components/ToneBadge';
import {
  ACTION_LABELS,
  ACTIONS_COLUMN,
  actionsFor,
  CONNECTED,
  CURRENT_BADGE,
  LABELS_COLUMN,
  lastSeen,
  NO_LABELS,
  NO_RUNNERS,
  OFFLINE,
  OUTDATED_BADGE,
  RUNNER_COLUMN,
  RUNNERS_TABLE_TITLE,
  RUNS_COLUMN,
  RunnerAction,
  SEEN_COLUMN,
  SERVING,
  STATE_COLUMN,
  stateLabel,
  stateTone,
  workReason,
} from '../messages';
import type { RunnerResponse } from '../types';

interface RunnersTableProps {
  runners: readonly RunnerResponse[];
  editable: boolean;
  now: Date;
  onAction(runner: RunnerResponse, action: RunnerAction): void;
}

const DANGEROUS: ReadonlySet<RunnerAction> = new Set([RunnerAction.Revoke, RunnerAction.Remove]);

function work(runner: RunnerResponse): string {
  if (runner.serving) {
    return SERVING;
  }
  return runner.code === null ? OFFLINE : workReason(runner.code);
}

function seen(runner: RunnerResponse, now: Date): string {
  return runner.connected ? CONNECTED : lastSeen(relativeTime(runner.last_seen_at, now));
}

function actionsOf(runner: RunnerResponse): RunnerAction[] {
  return runner.connected
    ? [...actionsFor(runner.state)]
    : [...actionsFor(runner.state), RunnerAction.Remove];
}

export function RunnersTable({ runners, editable, now, onAction }: RunnersTableProps) {
  const titleId = useId();

  return (
    <section className="settings-card runners-table-card" aria-labelledby={titleId}>
      <h3 id={titleId} className="runners-card-title">
        {RUNNERS_TABLE_TITLE}
      </h3>
      {runners.length === 0 ? (
        <p className="runners-hint">{NO_RUNNERS}</p>
      ) : (
        <Table className="runners-table" aria-labelledby={titleId}>
          <TableHeader>
            <TableRow>
              <TableHead scope="col">{RUNNER_COLUMN}</TableHead>
              <TableHead scope="col">{LABELS_COLUMN}</TableHead>
              <TableHead scope="col">{STATE_COLUMN}</TableHead>
              <TableHead scope="col">{SEEN_COLUMN}</TableHead>
              <TableHead scope="col" className="runner-runs">
                {RUNS_COLUMN}
              </TableHead>
              {editable && (
                <TableHead scope="col" className="runner-actions-head">
                  {ACTIONS_COLUMN}
                </TableHead>
              )}
            </TableRow>
          </TableHeader>
          <TableBody>
            {runners.map((runner) => (
              <TableRow key={runner.id} aria-label={runner.name}>
                <TableCell data-label={RUNNER_COLUMN}>
                  <div className="runner-stack">
                    <span className="runner-line">
                      <span className="runner-name">{runner.name}</span>
                      {runner.current && <Badge variant="accent">{CURRENT_BADGE}</Badge>}
                    </span>
                    <span className="runner-line">
                      <span className="runner-version">{runner.version}</span>
                      {runner.outdated && <Badge variant="warning">{OUTDATED_BADGE}</Badge>}
                    </span>
                  </div>
                </TableCell>
                <TableCell data-label={LABELS_COLUMN}>
                  {runner.labels.length === 0 ? (
                    <span className="runner-muted">{NO_LABELS}</span>
                  ) : (
                    <ul className="runner-labels" aria-label={`${LABELS_COLUMN} of ${runner.name}`}>
                      {runner.labels.map((label) => (
                        <li key={label} className="runner-label">
                          {label}
                        </li>
                      ))}
                    </ul>
                  )}
                </TableCell>
                <TableCell data-label={STATE_COLUMN}>
                  <div className="runner-stack">
                    <span className="runner-line">
                      <ToneBadge tone={stateTone(runner.state)}>
                        {stateLabel(runner.state)}
                      </ToneBadge>
                    </span>
                    <span
                      className={
                        runner.serving ? 'runner-work runner-work--serving' : 'runner-work'
                      }
                    >
                      {work(runner)}
                    </span>
                  </div>
                </TableCell>
                <TableCell data-label={SEEN_COLUMN}>
                  <span
                    className={
                      runner.connected ? 'runner-seen runner-seen--connected' : 'runner-seen'
                    }
                    title={absoluteTime(runner.last_seen_at)}
                  >
                    {seen(runner, now)}
                  </span>
                </TableCell>
                <TableCell className="runner-runs" data-label={RUNS_COLUMN}>
                  {runner.runs}
                </TableCell>
                {editable && (
                  <TableCell data-label={ACTIONS_COLUMN}>
                    <div className="runner-actions">
                      {actionsOf(runner).map((action) => (
                        <Button
                          key={action}
                          type="button"
                          variant="ghost"
                          size="sm"
                          className={DANGEROUS.has(action) ? 'runner-action--danger' : undefined}
                          aria-label={`${ACTION_LABELS[action]} ${runner.name}`}
                          onClick={() => onAction(runner, action)}
                        >
                          {ACTION_LABELS[action]}
                        </Button>
                      ))}
                    </div>
                  </TableCell>
                )}
              </TableRow>
            ))}
          </TableBody>
        </Table>
      )}
    </section>
  );
}
