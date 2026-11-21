import { Edition, LicenseStatus } from '../schemas';
import {
  type Budget,
  BudgetScope,
  BudgetState,
  type BudgetsResponse,
  type DayUsage,
  type NamedUsage,
  type SpendLicense,
  type SpendReport,
  type SpendReportResponse,
  UncountedReason,
  type Usage,
  type UserUsage,
  type WorkspaceUsage,
} from './schemas';

export const ORGANIZATION_ID = '9a8b7c6d-5e4f-4a3b-8c2d-1e0f9a8b7c6d';

export const PLATFORM_WORKSPACE_ID = '1f2e3d4c-5b6a-4978-8a6b-5c4d3e2f1a0b';

export const RESEARCH_WORKSPACE_ID = '2a3b4c5d-6e7f-4081-9a2b-3c4d5e6f7a8b';

export const ADA_ID = '3b4c5d6e-7f80-4192-8b3c-4d5e6f7a8b9c';

export const GRACE_ID = '4c5d6e7f-8091-42a3-9c4d-5e6f7a8b9cad';

export const ORGANIZATION_BUDGET_ID = '5d6e7f80-91a2-43b4-8d5e-6f7a8b9cadbe';

export const WORKSPACE_BUDGET_ID = '6e7f8091-a2b3-44c5-9e6f-7a8b9cadbecf';

export const PERSON_BUDGET_ID = '7f8091a2-b3c4-45d6-8f7a-8b9cadbecfd0';

export const FROM = '2026-11-01';

export const TO = '2026-11-21';

export const MONTH = '2026-11';

export const INGESTED_THROUGH = '2026-11-21T10:04:00Z';

export const UPDATED_AT = '2026-11-15T10:30:00Z';

export const EXPORT_FILENAME = `zone-spend-${FROM}-${TO}.csv`;

export const COMMUNITY_LICENSE: SpendLicense = {
  entitled: false,
  enforced: false,
  edition: Edition.Community,
  status: LicenseStatus.Community,
  editable: false,
};

export const ACTIVE_LICENSE: SpendLicense = {
  entitled: true,
  enforced: true,
  edition: Edition.Team,
  status: LicenseStatus.Active,
  editable: true,
};

export const LAPSED_LICENSE: SpendLicense = {
  entitled: false,
  enforced: true,
  edition: Edition.Team,
  status: LicenseStatus.Lapsed,
  editable: false,
};

const DAYS = 21;

const DAY_MILLISECONDS = 86_400_000;

const ZERO: Usage = { spend_microdollars: 0, requests: 0, prompt_tokens: 0, completion_tokens: 0 };

function dates(from: string, days: number): string[] {
  const start = Date.parse(`${from}T00:00:00Z`);
  return Array.from({ length: days }, (_, index) =>
    new Date(start + index * DAY_MILLISECONDS).toISOString().slice(0, 10)
  );
}

function sum(rows: readonly Usage[]): Usage {
  return rows.reduce(
    (total, row) => ({
      spend_microdollars: total.spend_microdollars + row.spend_microdollars,
      requests: total.requests + row.requests,
      prompt_tokens: total.prompt_tokens + row.prompt_tokens,
      completion_tokens: total.completion_tokens + row.completion_tokens,
    }),
    ZERO
  );
}

function share(total: Usage, weights: readonly number[]): Usage[] {
  const whole = weights.reduce((left, right) => left + right, 0);
  const parts = weights.map((weight) => ({
    spend_microdollars: Math.floor((total.spend_microdollars * weight) / whole),
    requests: Math.floor((total.requests * weight) / whole),
    prompt_tokens: Math.floor((total.prompt_tokens * weight) / whole),
    completion_tokens: Math.floor((total.completion_tokens * weight) / whole),
  }));
  const rest = sum(parts.slice(1));
  parts[0] = {
    spend_microdollars: total.spend_microdollars - rest.spend_microdollars,
    requests: total.requests - rest.requests,
    prompt_tokens: total.prompt_tokens - rest.prompt_tokens,
    completion_tokens: total.completion_tokens - rest.completion_tokens,
  };
  return parts;
}

const WEEKEND = new Set([0, 6]);

function dayUsage(date: string, index: number): DayUsage {
  const quiet = WEEKEND.has(new Date(`${date}T00:00:00Z`).getUTCDay());
  const requests = quiet ? 40 + index * 3 : 220 + ((index * 37) % 140);
  return {
    date,
    spend_microdollars: requests * (quiet ? 9_150 : 14_275),
    requests,
    prompt_tokens: requests * 2_340,
    completion_tokens: requests * 610,
  };
}

const LOCAL: Usage = {
  spend_microdollars: 0,
  requests: 412,
  prompt_tokens: 913_000,
  completion_tokens: 201_000,
};

const PRICED_DAILY = dates(FROM, DAYS).map(dayUsage);

const PRICED_TOTAL = sum(PRICED_DAILY);

const LAST_DAY = PRICED_DAILY.length - 1;

export const POPULATED_DAILY: DayUsage[] = PRICED_DAILY.map((day, index) =>
  index === LAST_DAY ? { date: day.date, ...sum([day, LOCAL]) } : day
);

const POPULATED_TOTAL = sum(POPULATED_DAILY);

const [PLATFORM, RESEARCH, DELETED] = share(POPULATED_TOTAL, [62, 33, 5]);

export const POPULATED_WORKSPACES: WorkspaceUsage[] = [
  { id: PLATFORM_WORKSPACE_ID, name: 'Platform', ...PLATFORM },
  { id: RESEARCH_WORKSPACE_ID, name: 'Research', ...RESEARCH },
  { id: null, name: null, ...DELETED },
];

const [ADA, AUTOMATION, GRACE, FORMER] = share(POPULATED_TOTAL, [48, 31, 15, 6]);

export const POPULATED_USERS: UserUsage[] = [
  { id: ADA_ID, name: 'Ada Lovelace', email: 'ada@example.com', automated: false, ...ADA },
  { id: null, name: null, email: null, automated: true, ...AUTOMATION },
  { id: GRACE_ID, name: null, email: 'grace@example.com', automated: false, ...GRACE },
  { id: null, name: null, email: null, automated: false, ...FORMER },
];

const [ANTHROPIC, OPENAI] = share(PRICED_TOTAL, [71, 29]);

export const POPULATED_MODELS: NamedUsage[] = [
  { name: 'claude-sonnet-4', ...ANTHROPIC },
  { name: 'gpt-4o-mini', ...OPENAI },
  { name: 'qwen3:8b', ...LOCAL },
];

export const POPULATED_PROVIDERS: NamedUsage[] = [
  { name: 'anthropic', ...ANTHROPIC },
  { name: 'openai', ...OPENAI },
  { name: 'ollama', ...LOCAL },
];

export const POPULATED_REPORT: SpendReport = {
  from: FROM,
  to: TO,
  ingested_through: INGESTED_THROUGH,
  total: POPULATED_TOTAL,
  daily: POPULATED_DAILY,
  workspaces: POPULATED_WORKSPACES,
  users: POPULATED_USERS,
  models: POPULATED_MODELS,
  providers: POPULATED_PROVIDERS,
  uncounted: [UncountedReason.CodingAgent, UncountedReason.DirectProvider],
};

export const EMPTY_REPORT: SpendReport = {
  from: FROM,
  to: TO,
  ingested_through: null,
  total: ZERO,
  daily: dates(FROM, DAYS).map((date) => ({ date, ...ZERO })),
  workspaces: [],
  users: [],
  models: [],
  providers: [],
  uncounted: [],
};

export const UNLICENSED_REPORT: SpendReportResponse = { ...COMMUNITY_LICENSE, report: null };

export const ACTIVE_REPORT: SpendReportResponse = { ...ACTIVE_LICENSE, report: POPULATED_REPORT };

export const LAPSED_REPORT: SpendReportResponse = { ...LAPSED_LICENSE, report: POPULATED_REPORT };

export const ACTIVE_EMPTY_REPORT: SpendReportResponse = { ...ACTIVE_LICENSE, report: EMPTY_REPORT };

export const ORGANIZATION_BUDGET: Budget = {
  id: ORGANIZATION_BUDGET_ID,
  scope: BudgetScope.Organization,
  workspace_id: null,
  workspace_name: null,
  user_id: null,
  user_name: null,
  user_email: null,
  limit_microdollars: 250_000_000,
  threshold_percent: 80,
  hard: false,
  spent_microdollars: POPULATED_TOTAL.spend_microdollars,
  state: BudgetState.Under,
  updated_at: UPDATED_AT,
};

export const WORKSPACE_BUDGET: Budget = {
  id: WORKSPACE_BUDGET_ID,
  scope: BudgetScope.Workspace,
  workspace_id: PLATFORM_WORKSPACE_ID,
  workspace_name: 'Platform',
  user_id: null,
  user_name: null,
  user_email: null,
  limit_microdollars: 35_000_000,
  threshold_percent: 75,
  hard: true,
  spent_microdollars: PLATFORM.spend_microdollars,
  state: BudgetState.Exhausted,
  updated_at: UPDATED_AT,
};

export const PERSON_BUDGET: Budget = {
  id: PERSON_BUDGET_ID,
  scope: BudgetScope.User,
  workspace_id: null,
  workspace_name: null,
  user_id: ADA_ID,
  user_name: 'Ada Lovelace',
  user_email: 'ada@example.com',
  limit_microdollars: 40_000_000,
  threshold_percent: 90,
  hard: false,
  spent_microdollars: 36_500_000,
  state: BudgetState.Warning,
  updated_at: UPDATED_AT,
};

export const BUDGETS = [ORGANIZATION_BUDGET, WORKSPACE_BUDGET, PERSON_BUDGET];

export const UNLICENSED_BUDGETS: BudgetsResponse = {
  ...COMMUNITY_LICENSE,
  month: MONTH,
  budgets: [],
};

export const ACTIVE_BUDGETS: BudgetsResponse = {
  ...ACTIVE_LICENSE,
  month: MONTH,
  budgets: BUDGETS,
};

export const LAPSED_BUDGETS: BudgetsResponse = {
  ...LAPSED_LICENSE,
  month: MONTH,
  budgets: BUDGETS,
};

export const EMPTY_BUDGETS: BudgetsResponse = { ...ACTIVE_LICENSE, month: MONTH, budgets: [] };

export const WORKSPACES = [
  { id: PLATFORM_WORKSPACE_ID, name: 'Platform' },
  { id: RESEARCH_WORKSPACE_ID, name: 'Research' },
];

export const MEMBERS = [
  { user_id: ADA_ID, email: 'ada@example.com', display_name: 'Ada Lovelace' },
  { user_id: GRACE_ID, email: 'grace@example.com', display_name: null },
];
