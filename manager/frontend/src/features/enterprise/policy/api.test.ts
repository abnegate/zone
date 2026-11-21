import { afterAll, afterEach, describe, expect, it, spyOn } from 'bun:test';
import { ApiError } from '../../../api/ApiError';
import { session } from '../../../api/session';
import { policyApi } from './api';
import {
  ACTIVE_ORGANIZATION,
  ACTIVE_WORKSPACE_OVERRIDDEN,
  ORGANIZATION_ID,
  WORKSPACE_ID,
} from './fixtures';
import { PolicyRefusal } from './PolicyRefusal';
import { PolicyCode } from './schemas';

const fetchSpy = spyOn(globalThis, 'fetch');

const ORGANIZATION_URL = `/api/organizations/${ORGANIZATION_ID}/agent-policy`;

const WORKSPACE_URL = `/api/organizations/${ORGANIZATION_ID}/workspaces/${WORKSPACE_ID}/agent-policy`;

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

describe('policyApi.organization', () => {
  it('reads the organization policy with the session token', async () => {
    session.setAccessToken('token-1');
    const request = respond(ACTIVE_ORGANIZATION);

    expect(await policyApi.organization(ORGANIZATION_ID)).toEqual(ACTIVE_ORGANIZATION);
    expect(sent(request)).toEqual({ url: ORGANIZATION_URL, method: 'GET', body: undefined });
    expect(new Headers(request.mock.calls[0][1]?.headers).get('Authorization')).toBe(
      'Bearer token-1'
    );
  });

  it('escapes the organization id in the path', async () => {
    const request = respond(ACTIVE_ORGANIZATION);

    await policyApi.organization('a/b');

    expect(sent(request).url).toBe('/api/organizations/a%2Fb/agent-policy');
  });

  it('rejects a response outside the contract', async () => {
    respond({ ...ACTIVE_ORGANIZATION, policy: {} });

    expect(await failure(policyApi.organization(ORGANIZATION_ID))).toBeInstanceOf(Error);
  });

  it('throws a plain ApiError for a failure without a policy code', async () => {
    respond({ error: 'Organization not found' }, 404);

    const error = await failure(policyApi.organization(ORGANIZATION_ID));

    expect(error).toBeInstanceOf(ApiError);
    expect(error).not.toBeInstanceOf(PolicyRefusal);
    expect((error as ApiError).message).toBe(
      'Failed to load the agent policy: Organization not found'
    );
  });
});

describe('policyApi.updateOrganization', () => {
  it('patches only the keys it is given', async () => {
    const request = respond(ACTIVE_ORGANIZATION);

    await policyApi.updateOrganization(ORGANIZATION_ID, { domains: null, ceiling: 'write' });

    const { url, method, body } = sent(request);
    expect({ url, method }).toEqual({ url: ORGANIZATION_URL, method: 'PATCH' });
    expect(JSON.parse(String(body))).toEqual({ domains: null, ceiling: 'write' });
  });

  it('turns a coded refusal into a PolicyRefusal that keeps its status', async () => {
    respond({ error: 'A license is required', code: PolicyCode.LicenseRequired }, 403);

    const error = await failure(policyApi.updateOrganization(ORGANIZATION_ID, { models: [] }));

    expect(error).toBeInstanceOf(PolicyRefusal);
    expect((error as PolicyRefusal).code).toBe(PolicyCode.LicenseRequired);
    expect((error as PolicyRefusal).status).toBe(403);
  });

  it('turns a validation refusal into a PolicyRefusal', async () => {
    respond({ error: 'Invalid model pattern', code: PolicyCode.ModelInvalid }, 400);

    const error = await failure(policyApi.updateOrganization(ORGANIZATION_ID, { models: ['a b'] }));

    expect((error as PolicyRefusal).code).toBe(PolicyCode.ModelInvalid);
    expect((error as PolicyRefusal).status).toBe(400);
  });
});

describe('policyApi.workspace', () => {
  it('reads the workspace policy', async () => {
    const request = respond(ACTIVE_WORKSPACE_OVERRIDDEN);

    expect(await policyApi.workspace(ORGANIZATION_ID, WORKSPACE_ID)).toEqual(
      ACTIVE_WORKSPACE_OVERRIDDEN
    );
    expect(sent(request)).toEqual({ url: WORKSPACE_URL, method: 'GET', body: undefined });
  });

  it('patches the workspace policy and reads back the workspace shape', async () => {
    const request = respond(ACTIVE_WORKSPACE_OVERRIDDEN);

    expect(
      await policyApi.updateWorkspace(ORGANIZATION_ID, WORKSPACE_ID, { providers: null })
    ).toEqual(ACTIVE_WORKSPACE_OVERRIDDEN);
    expect(sent(request)).toEqual({
      url: WORKSPACE_URL,
      method: 'PATCH',
      body: JSON.stringify({ providers: null }),
    });
  });

  it('refuses a widening workspace change with its code', async () => {
    respond({ error: 'Wider than the organization', code: PolicyCode.Wider }, 400);

    const error = await failure(
      policyApi.updateWorkspace(ORGANIZATION_ID, WORKSPACE_ID, { providers: ['openai'] })
    );

    expect((error as PolicyRefusal).code).toBe(PolicyCode.Wider);
  });
});
