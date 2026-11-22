import { afterAll, beforeAll, beforeEach, describe, expect, it, mock } from 'bun:test';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { act, renderHook, waitFor } from '@testing-library/react';
import { createElement, type ReactNode } from 'react';
import { ApiError } from '../../../../api/ApiError';
import {
  ACTIVE_BUDGETS,
  ACTIVE_REPORT,
  FROM,
  ORGANIZATION_BUDGET,
  ORGANIZATION_ID,
  TO,
} from '../fixtures';
import { spendQueryKeys } from '../queryKeys';
import { SpendRefusal } from '../SpendRefusal';
import { SpendCode } from '../schemas';

const report = mock();
const budgets = mock();
const createBudget = mock();
const updateBudget = mock();
const deleteBudget = mock();

mock.module('../api', () => ({
  spendApi: { report, download: mock(), budgets, createBudget, updateBudget, deleteBudget },
}));

let hooks: typeof import('./index');

beforeAll(async () => {
  hooks = await import('./index');
});

afterAll(() => {
  mock.restore();
});

const QUERY = { from: FROM, to: TO };

function client(): QueryClient {
  return new QueryClient({
    defaultOptions: { queries: { retry: false, gcTime: Infinity }, mutations: { retry: false } },
  });
}

function wrapper(queryClient: QueryClient) {
  return ({ children }: { children: ReactNode }) =>
    createElement(QueryClientProvider, { client: queryClient }, children);
}

beforeEach(() => {
  for (const fn of [report, budgets, createBudget, updateBudget, deleteBudget]) {
    fn.mockReset();
  }
});

describe('useSpendReport', () => {
  it('loads the report for the query', async () => {
    report.mockResolvedValue(ACTIVE_REPORT);
    const { result } = renderHook(() => hooks.useSpendReport(ORGANIZATION_ID, QUERY), {
      wrapper: wrapper(client()),
    });

    await waitFor(() => expect(result.current.response).toEqual(ACTIVE_REPORT));
    expect(report).toHaveBeenCalledWith(ORGANIZATION_ID, QUERY);
  });

  it('keeps the previous report on screen while a new range loads', async () => {
    report.mockResolvedValueOnce(ACTIVE_REPORT).mockImplementationOnce(() => new Promise(() => {}));
    const { result, rerender } = renderHook(
      ({ query }) => hooks.useSpendReport(ORGANIZATION_ID, query),
      { wrapper: wrapper(client()), initialProps: { query: QUERY } }
    );
    await waitFor(() => expect(result.current.response).toEqual(ACTIVE_REPORT));

    rerender({ query: { ...QUERY, from: '2026-11-10' } });

    await waitFor(() => expect(result.current.refreshing).toBe(true));
    expect(result.current.response).toEqual(ACTIVE_REPORT);
    expect(result.current.loading).toBe(false);
  });

  it('reports a refused load without retrying it', async () => {
    report.mockRejectedValue(new ApiError('Failed to load spend: Forbidden', 403));
    const { result } = renderHook(() => hooks.useSpendReport(ORGANIZATION_ID, QUERY), {
      wrapper: wrapper(client()),
    });

    await waitFor(() => expect(result.current.error).toBe('Failed to load spend: Forbidden'));
    expect(report).toHaveBeenCalledTimes(1);
  });
});

describe('useBudgets', () => {
  it('loads the budgets of the organization', async () => {
    budgets.mockResolvedValue(ACTIVE_BUDGETS);
    const { result } = renderHook(() => hooks.useBudgets(ORGANIZATION_ID), {
      wrapper: wrapper(client()),
    });

    await waitFor(() => expect(result.current.response).toEqual(ACTIVE_BUDGETS));
  });
});

describe('useBudgetMutations', () => {
  it('refreshes the budgets after each change', async () => {
    createBudget.mockResolvedValue(ORGANIZATION_BUDGET);
    updateBudget.mockResolvedValue(ORGANIZATION_BUDGET);
    deleteBudget.mockResolvedValue(undefined);
    const queryClient = client();
    const key = spendQueryKeys.budgets(ORGANIZATION_ID);
    const { result } = renderHook(() => hooks.useBudgetMutations(ORGANIZATION_ID), {
      wrapper: wrapper(queryClient),
    });
    const created = mock();
    const updated = mock();
    const removed = mock();

    for (const run of [
      () => result.current.create.run({ scope: 'organization', limit_microdollars: 1 }, created),
      () => result.current.update.run(ORGANIZATION_BUDGET.id, { hard: true }, updated),
      () => result.current.remove.run(ORGANIZATION_BUDGET.id, removed),
    ]) {
      queryClient.setQueryData(key, ACTIVE_BUDGETS);
      act(run);
      await waitFor(() => expect(queryClient.getQueryState(key)?.isInvalidated).toBe(true));
    }

    expect(created).toHaveBeenCalledWith(ORGANIZATION_BUDGET);
    expect(updated).toHaveBeenCalledWith(ORGANIZATION_BUDGET);
    expect(removed).toHaveBeenCalledTimes(1);
    expect(updateBudget).toHaveBeenCalledWith(ORGANIZATION_ID, ORGANIZATION_BUDGET.id, {
      hard: true,
    });
  });

  it('exposes a refusal and resets it', async () => {
    createBudget.mockRejectedValue(
      new SpendRefusal({ error: 'raw', code: SpendCode.LicenseRequired }, 403)
    );
    const { result } = renderHook(() => hooks.useBudgetMutations(ORGANIZATION_ID), {
      wrapper: wrapper(client()),
    });

    act(() => result.current.create.run({ scope: 'organization', limit_microdollars: 1 }));

    await waitFor(() =>
      expect(result.current.create.refusal?.code).toBe(SpendCode.LicenseRequired)
    );
    expect(result.current.create.error).toBe('raw');

    act(() => result.current.create.reset());
    await waitFor(() => expect(result.current.create.refusal).toBeNull());
  });

  it('refreshes the budgets after a refusal, since a license refusal means the license changed', async () => {
    const refusal = new SpendRefusal({ error: 'raw', code: SpendCode.LicenseRequired }, 403);
    createBudget.mockRejectedValue(refusal);
    updateBudget.mockRejectedValue(refusal);
    deleteBudget.mockRejectedValue(refusal);
    const queryClient = client();
    const key = spendQueryKeys.budgets(ORGANIZATION_ID);
    const { result } = renderHook(() => hooks.useBudgetMutations(ORGANIZATION_ID), {
      wrapper: wrapper(queryClient),
    });
    const done = mock();

    for (const run of [
      () => result.current.create.run({ scope: 'organization', limit_microdollars: 1 }, done),
      () => result.current.update.run(ORGANIZATION_BUDGET.id, { hard: true }, done),
      () => result.current.remove.run(ORGANIZATION_BUDGET.id, done),
    ]) {
      queryClient.setQueryData(key, ACTIVE_BUDGETS);
      act(run);
      await waitFor(() => expect(queryClient.getQueryState(key)?.isInvalidated).toBe(true));
    }

    expect(done).not.toHaveBeenCalled();
  });
});

describe('spendQueryKeys', () => {
  it('keys spend under the organization so a range change keeps the previous report', () => {
    expect(spendQueryKeys.report('o', QUERY)).toEqual(['spend', 'reports', 'o', QUERY]);
    expect(spendQueryKeys.budgets('o')).toEqual(['spend', 'budgets', 'o']);
    expect(spendQueryKeys.workspaces('o')).toEqual(['spend', 'workspaces', 'o']);
    expect(spendQueryKeys.members('o')).toEqual(['spend', 'members', 'o']);
  });
});
