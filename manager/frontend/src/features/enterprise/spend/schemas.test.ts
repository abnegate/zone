import { describe, expect, it } from 'bun:test';
import {
  ACTIVE_BUDGETS,
  ACTIVE_EMPTY_REPORT,
  ACTIVE_REPORT,
  BUDGETS,
  LAPSED_BUDGETS,
  LAPSED_REPORT,
  ORGANIZATION_BUDGET,
  POPULATED_REPORT,
  UNLICENSED_BUDGETS,
  UNLICENSED_REPORT,
  WORKSPACE_BUDGET,
} from './fixtures';
import {
  BUDGET_SCOPES,
  BUDGET_STATES,
  BudgetSchema,
  BudgetState,
  BudgetsResponseSchema,
  CreateBudgetRequestSchema,
  SPEND_CODES,
  SPEND_REFUSAL_CODES,
  SpendQuerySchema,
  SpendRefusalSchema,
  SpendReportResponseSchema,
  UNCOUNTED_REASONS,
  UpdateBudgetRequestSchema,
  type Usage,
} from './schemas';

function total(rows: readonly Usage[]): Usage {
  return rows.reduce(
    (sum, row) => ({
      spend_microdollars: sum.spend_microdollars + row.spend_microdollars,
      requests: sum.requests + row.requests,
      prompt_tokens: sum.prompt_tokens + row.prompt_tokens,
      completion_tokens: sum.completion_tokens + row.completion_tokens,
    }),
    { spend_microdollars: 0, requests: 0, prompt_tokens: 0, completion_tokens: 0 }
  );
}

describe('spend constants', () => {
  it('names the contract codes, scopes, states and uncounted reasons', () => {
    expect([...SPEND_REFUSAL_CODES]).toEqual([
      'spend_body_invalid',
      'spend_range_invalid',
      'budget_limit_invalid',
      'budget_threshold_invalid',
      'budget_scope_invalid',
      'budget_exists',
      'license_required',
    ]);
    expect([...SPEND_CODES]).toEqual([...SPEND_REFUSAL_CODES, 'budget_exhausted']);
    expect([...BUDGET_SCOPES]).toEqual(['organization', 'workspace', 'user']);
    expect([...BUDGET_STATES]).toEqual(['under', 'warning', 'exhausted']);
    expect([...UNCOUNTED_REASONS]).toEqual([
      'coding_agent',
      'direct_provider',
      'external_endpoint',
      'saved_key',
    ]);
  });
});

describe('SpendReportResponseSchema', () => {
  it('accepts every report fixture', () => {
    for (const fixture of [UNLICENSED_REPORT, ACTIVE_REPORT, LAPSED_REPORT, ACTIVE_EMPTY_REPORT]) {
      expect(SpendReportResponseSchema.parse(fixture)).toEqual(fixture);
    }
  });

  it('accepts a report the server has not ingested into yet', () => {
    const report = { ...POPULATED_REPORT, ingested_through: null };
    expect(SpendReportResponseSchema.parse({ ...ACTIVE_REPORT, report }).report).toEqual(report);
  });

  it('accepts deleted workspaces and automation as null ids', () => {
    const parsed = SpendReportResponseSchema.parse(ACTIVE_REPORT);
    expect(parsed.report?.workspaces.some((row) => row.id === null)).toBe(true);
    expect(parsed.report?.users.some((row) => row.id === null)).toBe(true);
  });

  it('refuses money that is not whole microdollars', () => {
    const total = { ...POPULATED_REPORT.total, spend_microdollars: 1.5 };
    expect(
      SpendReportResponseSchema.safeParse({
        ...ACTIVE_REPORT,
        report: { ...POPULATED_REPORT, total },
      }).success
    ).toBe(false);
  });

  it('refuses an uncounted reason outside the contract', () => {
    const report = { ...POPULATED_REPORT, uncounted: ['embeddings'] };
    expect(SpendReportResponseSchema.safeParse({ ...ACTIVE_REPORT, report }).success).toBe(false);
  });

  it('refuses a response without the license fields', () => {
    const { editable: _, ...rest } = ACTIVE_REPORT;
    expect(SpendReportResponseSchema.safeParse(rest).success).toBe(false);
  });

  it('accepts an edition outside the known ones, since it never reads one', () => {
    expect(
      SpendReportResponseSchema.safeParse({ ...ACTIVE_REPORT, edition: 'galaxy' }).success
    ).toBe(true);
    expect(BudgetsResponseSchema.safeParse({ ...ACTIVE_BUDGETS, edition: 'galaxy' }).success).toBe(
      true
    );
  });

  it('keeps the populated fixture consistent with itself', () => {
    expect(total(POPULATED_REPORT.daily)).toEqual(POPULATED_REPORT.total);
    for (const rows of [
      POPULATED_REPORT.workspaces,
      POPULATED_REPORT.users,
      POPULATED_REPORT.models,
      POPULATED_REPORT.providers,
    ]) {
      expect(total(rows)).toEqual(POPULATED_REPORT.total);
    }
  });
});

describe('BudgetsResponseSchema', () => {
  it('accepts every budgets fixture', () => {
    for (const fixture of [UNLICENSED_BUDGETS, ACTIVE_BUDGETS, LAPSED_BUDGETS]) {
      expect(BudgetsResponseSchema.parse(fixture)).toEqual(fixture);
    }
  });

  it('refuses a month that is not YYYY-MM', () => {
    expect(
      BudgetsResponseSchema.safeParse({ ...ACTIVE_BUDGETS, month: '2026-11-01' }).success
    ).toBe(false);
  });

  it('refuses a threshold outside 1 to 100 and a limit of zero', () => {
    expect(BudgetSchema.safeParse({ ...ORGANIZATION_BUDGET, threshold_percent: 0 }).success).toBe(
      false
    );
    expect(BudgetSchema.safeParse({ ...ORGANIZATION_BUDGET, threshold_percent: 101 }).success).toBe(
      false
    );
    expect(BudgetSchema.safeParse({ ...ORGANIZATION_BUDGET, limit_microdollars: 0 }).success).toBe(
      false
    );
  });

  it('keeps each fixture budget in the state its spend implies', () => {
    for (const budget of BUDGETS) {
      const spent = budget.spent_microdollars;
      const expected =
        spent >= budget.limit_microdollars
          ? BudgetState.Exhausted
          : spent * 100 >= budget.limit_microdollars * budget.threshold_percent
            ? BudgetState.Warning
            : BudgetState.Under;
      expect(budget.state, budget.id).toBe(expected);
    }
  });
});

describe('request schemas', () => {
  it('accepts an organization budget with only a limit', () => {
    expect(
      CreateBudgetRequestSchema.parse({ scope: 'organization', limit_microdollars: 1 })
    ).toEqual({ scope: 'organization', limit_microdollars: 1 });
  });

  it('refuses unknown fields so nothing extra reaches the server', () => {
    expect(
      CreateBudgetRequestSchema.safeParse({
        scope: 'organization',
        limit_microdollars: 1,
        spent_microdollars: 0,
      }).success
    ).toBe(false);
    expect(UpdateBudgetRequestSchema.safeParse({ scope: 'user' }).success).toBe(false);
  });

  it('accepts an empty patch and refuses a fractional threshold', () => {
    expect(UpdateBudgetRequestSchema.parse({})).toEqual({});
    expect(UpdateBudgetRequestSchema.safeParse({ threshold_percent: 50.5 }).success).toBe(false);
  });

  it('refuses a query whose dates are not YYYY-MM-DD', () => {
    expect(SpendQuerySchema.safeParse({ from: '2026-11-1', to: '2026-11-21' }).success).toBe(false);
    expect(
      SpendQuerySchema.safeParse({
        from: '2026-11-01',
        to: '2026-11-21',
        workspace_id: WORKSPACE_BUDGET.workspace_id,
      }).success
    ).toBe(true);
  });
});

describe('SpendRefusalSchema', () => {
  it('accepts each refusal code and refuses the blocked-request code', () => {
    for (const code of SPEND_REFUSAL_CODES) {
      expect(SpendRefusalSchema.parse({ error: 'raw', code }).code).toBe(code);
    }
    expect(SpendRefusalSchema.safeParse({ error: 'raw', code: 'budget_exhausted' }).success).toBe(
      false
    );
    expect(SpendRefusalSchema.safeParse({ error: 'raw' }).success).toBe(false);
  });
});
