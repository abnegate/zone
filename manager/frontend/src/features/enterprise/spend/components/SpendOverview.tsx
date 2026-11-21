import { SectionCard } from '../../../analytics/components/SectionCard';
import { StatTile } from '../../../analytics/components/StatTile';
import { momentLabel } from '../calendar';
import {
  AUTOMATION_DETAIL,
  CHART_TITLE,
  COMPLETION_TOKENS_TILE,
  COUNTED_THROUGH,
  MODELS_TITLE,
  NOTHING_COUNTED,
  PEOPLE_TITLE,
  PROMPT_TOKENS_TILE,
  PROVIDERS_TITLE,
  REQUESTS_TILE,
  SPEND_TILE,
  WORKSPACES_TITLE,
} from '../labels';
import { personName, providerName, workspaceName } from '../messages';
import { count, exactMoney, money } from '../money';
import type { SpendReport } from '../schemas';
import { type BreakdownRow, SpendBreakdown } from './SpendBreakdown';
import { SpendDailyChart } from './SpendDailyChart';
import { SpendUncounted } from './SpendUncounted';

const DELETED_KEY = 'deleted';

const AUTOMATION_KEY = 'automation';

const FORMER_KEY = 'former';

function workspaceRows(report: SpendReport): BreakdownRow[] {
  return report.workspaces.map(({ id, name, ...usage }) => ({
    key: id ?? DELETED_KEY,
    name: workspaceName({ id, name }),
    muted: id === null,
    usage,
  }));
}

function personKey(id: string | null, automated: boolean): string {
  if (automated) return AUTOMATION_KEY;
  return id ?? FORMER_KEY;
}

function personDetail(
  name: string | null,
  email: string | null,
  automated: boolean
): string | undefined {
  if (automated) return AUTOMATION_DETAIL;
  return name ? (email ?? undefined) : undefined;
}

function personRows(report: SpendReport): BreakdownRow[] {
  return report.users.map(({ id, name, email, automated, ...usage }) => ({
    key: personKey(id, automated),
    name: personName({ id, name, email, automated }),
    detail: personDetail(name, email, automated),
    muted: id === null,
    usage,
  }));
}

function namedRows(
  rows: SpendReport['models'],
  label: (name: string) => string = (name) => name
): BreakdownRow[] {
  return rows.map(({ name, ...usage }) => ({ key: name, name: label(name), usage }));
}

interface SpendOverviewProps {
  report: SpendReport;
  refreshing: boolean;
}

export function SpendOverview({ report, refreshing }: SpendOverviewProps) {
  return (
    <div className="spend-overview" aria-busy={refreshing}>
      <p className="spend-hint">
        {report.ingested_through
          ? `${COUNTED_THROUGH} ${momentLabel(report.ingested_through)}`
          : NOTHING_COUNTED}
      </p>
      <div className="spend-tiles">
        <StatTile
          label={SPEND_TILE}
          value={money(report.total.spend_microdollars)}
          detail={exactMoney(report.total.spend_microdollars)}
        />
        <StatTile label={REQUESTS_TILE} value={count(report.total.requests)} />
        <StatTile label={PROMPT_TOKENS_TILE} value={count(report.total.prompt_tokens)} />
        <StatTile label={COMPLETION_TOKENS_TILE} value={count(report.total.completion_tokens)} />
      </div>
      <SectionCard title={CHART_TITLE} wide>
        <SpendDailyChart daily={report.daily} />
      </SectionCard>
      <div className="spend-grid">
        <SpendBreakdown title={WORKSPACES_TITLE} rows={workspaceRows(report)} />
        <SpendBreakdown title={PEOPLE_TITLE} rows={personRows(report)} />
        <SpendBreakdown title={MODELS_TITLE} rows={namedRows(report.models)} />
        <SpendBreakdown title={PROVIDERS_TITLE} rows={namedRows(report.providers, providerName)} />
      </div>
      <SpendUncounted reasons={report.uncounted} />
    </div>
  );
}
