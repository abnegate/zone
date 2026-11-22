import { afterAll, afterEach, describe, expect, it, spyOn } from 'bun:test';
import { ApiError } from '../../../api/ApiError';
import { session } from '../../../api/session';
import { exportFilename, spendApi } from './api';
import {
  ACTIVE_BUDGETS,
  ACTIVE_REPORT,
  ADA_ID,
  EXPORT_FILENAME,
  FROM,
  ORGANIZATION_BUDGET,
  ORGANIZATION_ID,
  PLATFORM_WORKSPACE_ID,
  TO,
} from './fixtures';
import { DOWNLOAD_INTERRUPTED } from './labels';
import { SpendRefusal } from './SpendRefusal';
import { SpendCode } from './schemas';

const fetchSpy = spyOn(globalThis, 'fetch');

const BASE = `/api/organizations/${ORGANIZATION_ID}`;

const QUERY = { from: FROM, to: TO };

function respond(body: unknown, status = 200) {
  return fetchSpy.mockImplementation(async () => Response.json(body, { status }));
}

function sent(request: ReturnType<typeof respond>) {
  const [url, init] = request.mock.calls[0];
  return { url, method: init?.method, body: init?.body ?? undefined };
}

async function failure(action: Promise<unknown>): Promise<unknown> {
  try {
    await action;
  } catch (error) {
    return error;
  }
  throw new Error('expected the request to fail');
}

afterEach(() => {
  fetchSpy.mockReset();
  session.setAccessToken(null);
});

afterAll(() => {
  fetchSpy.mockRestore();
});

describe('spendApi.report', () => {
  it('reads the report for the range with the session token', async () => {
    session.setAccessToken('token-1');
    const request = respond(ACTIVE_REPORT);

    expect(await spendApi.report(ORGANIZATION_ID, QUERY)).toEqual(ACTIVE_REPORT);
    expect(sent(request)).toEqual({
      url: `${BASE}/spend-reports?from=${FROM}&to=${TO}`,
      method: 'GET',
      body: undefined,
    });
    expect(new Headers(request.mock.calls[0][1]?.headers).get('Authorization')).toBe(
      'Bearer token-1'
    );
  });

  it('adds only the filters that are set', async () => {
    const request = respond(ACTIVE_REPORT);

    await spendApi.report(ORGANIZATION_ID, {
      ...QUERY,
      workspace_id: PLATFORM_WORKSPACE_ID,
      user_id: ADA_ID,
    });

    expect(sent(request).url).toBe(
      `${BASE}/spend-reports?from=${FROM}&to=${TO}&workspace_id=${PLATFORM_WORKSPACE_ID}&user_id=${ADA_ID}`
    );
  });

  it('escapes the organization id in the path', async () => {
    const request = respond(ACTIVE_REPORT);

    await spendApi.report('a/b', QUERY);

    expect(sent(request).url).toStartWith('/api/organizations/a%2Fb/spend-reports?');
  });

  it('refuses to send a malformed range', async () => {
    expect(
      await failure(spendApi.report(ORGANIZATION_ID, { from: 'yesterday', to: TO }))
    ).toBeInstanceOf(Error);
    expect(fetchSpy).not.toHaveBeenCalled();
  });

  it('turns a coded refusal into a SpendRefusal', async () => {
    respond({ error: 'bad range', code: SpendCode.RangeInvalid }, 400);

    const error = await failure(spendApi.report(ORGANIZATION_ID, QUERY));

    expect(error).toBeInstanceOf(SpendRefusal);
    expect((error as SpendRefusal).code).toBe(SpendCode.RangeInvalid);
    expect((error as SpendRefusal).status).toBe(400);
  });

  it('keeps an uncoded refusal as an ApiError with the server message', async () => {
    respond({ error: 'This needs the spend:read permission in this organization.' }, 403);

    const error = await failure(spendApi.report(ORGANIZATION_ID, QUERY));

    expect(error).not.toBeInstanceOf(SpendRefusal);
    expect(error).toBeInstanceOf(ApiError);
    expect((error as ApiError).message).toBe(
      'Failed to load spend: This needs the spend:read permission in this organization.'
    );
  });

  it('rejects a response outside the contract', async () => {
    respond({ ...ACTIVE_REPORT, report: { from: FROM } });

    expect(await failure(spendApi.report(ORGANIZATION_ID, QUERY))).toBeInstanceOf(Error);
  });
});

describe('spendApi.download', () => {
  it('names the file from the Content-Disposition header', async () => {
    fetchSpy.mockImplementation(
      async () =>
        new Response('date\r\n', {
          headers: {
            'Content-Type': 'text/csv; charset=utf-8',
            'Content-Disposition': 'attachment; filename="zone-spend-2026-11-01-2026-11-21.csv"',
          },
        })
    );

    const file = await spendApi.download(ORGANIZATION_ID, QUERY);

    expect(file.filename).toBe('zone-spend-2026-11-01-2026-11-21.csv');
    expect(await file.blob.text()).toBe('date\r\n');
    expect(fetchSpy.mock.calls[0][0]).toBe(`${BASE}/spend-exports?from=${FROM}&to=${TO}`);
  });

  it('falls back to the contract filename without a disposition', async () => {
    fetchSpy.mockImplementation(async () => new Response('date\r\n'));

    expect((await spendApi.download(ORGANIZATION_ID, QUERY)).filename).toBe(EXPORT_FILENAME);
    expect(exportFilename(QUERY)).toBe(EXPORT_FILENAME);
  });

  it('reports an interrupted body', async () => {
    const response = new Response('date');
    spyOn(response, 'blob').mockRejectedValue(new TypeError('network'));
    fetchSpy.mockImplementation(async () => response);

    const error = await failure(spendApi.download(ORGANIZATION_ID, QUERY));

    expect((error as ApiError).message).toBe(DOWNLOAD_INTERRUPTED);
  });

  it('turns license_required into a SpendRefusal', async () => {
    respond({ error: 'raw', code: SpendCode.LicenseRequired }, 403);

    const error = await failure(spendApi.download(ORGANIZATION_ID, QUERY));

    expect((error as SpendRefusal).code).toBe(SpendCode.LicenseRequired);
  });
});

describe('spendApi budgets', () => {
  it('lists the budgets', async () => {
    const request = respond(ACTIVE_BUDGETS);

    expect(await spendApi.budgets(ORGANIZATION_ID)).toEqual(ACTIVE_BUDGETS);
    expect(sent(request)).toEqual({ url: `${BASE}/budgets`, method: 'GET', body: undefined });
  });

  it('creates a budget with the validated body', async () => {
    const request = respond(ORGANIZATION_BUDGET, 201);
    const body = { scope: 'organization' as const, limit_microdollars: 250_000_000, hard: false };

    expect(await spendApi.createBudget(ORGANIZATION_ID, body)).toEqual(ORGANIZATION_BUDGET);
    expect(sent(request)).toEqual({
      url: `${BASE}/budgets`,
      method: 'POST',
      body: JSON.stringify(body),
    });
  });

  it('patches only the fields it is given', async () => {
    const request = respond(ORGANIZATION_BUDGET);

    await spendApi.updateBudget(ORGANIZATION_ID, 'b/1', { threshold_percent: 50 });

    expect(sent(request)).toEqual({
      url: `${BASE}/budgets/b%2F1`,
      method: 'PATCH',
      body: JSON.stringify({ threshold_percent: 50 }),
    });
  });

  it('deletes a budget without reading a body', async () => {
    const request = fetchSpy.mockImplementation(async () => new Response(null, { status: 204 }));

    expect(await spendApi.deleteBudget(ORGANIZATION_ID, ORGANIZATION_BUDGET.id)).toBeUndefined();
    expect(sent(request)).toEqual({
      url: `${BASE}/budgets/${ORGANIZATION_BUDGET.id}`,
      method: 'DELETE',
      body: undefined,
    });
  });

  it('turns budget_exists into a SpendRefusal', async () => {
    respond({ error: 'exists', code: SpendCode.BudgetExists }, 409);

    const error = await failure(
      spendApi.createBudget(ORGANIZATION_ID, { scope: 'organization', limit_microdollars: 1 })
    );

    expect((error as SpendRefusal).code).toBe(SpendCode.BudgetExists);
    expect((error as SpendRefusal).status).toBe(409);
  });

  it('refuses to send a limit of zero', async () => {
    expect(
      await failure(
        spendApi.createBudget(ORGANIZATION_ID, { scope: 'organization', limit_microdollars: 0 })
      )
    ).toBeInstanceOf(Error);
    expect(fetchSpy).not.toHaveBeenCalled();
  });
});
