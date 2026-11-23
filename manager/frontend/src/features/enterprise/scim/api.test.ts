import { afterAll, afterEach, describe, expect, it, spyOn } from 'bun:test';
import { ApiError } from '../../../api/ApiError';
import { session } from '../../../api/session';
import { AUDITOR } from '../roles/fixtures';
import { scimApi } from './api';
import { ACTIVE, ENGINEERING, ISSUED, OKTA, ORGANIZATION_ID, ROTATED } from './fixtures';
import { ScimRefusal } from './ScimRefusal';
import { MappedRole, ScimCode } from './schemas';

const fetchSpy = spyOn(globalThis, 'fetch');

const SCIM_URL = `/api/organizations/${ORGANIZATION_ID}/scim`;

const TOKEN_URL = `${SCIM_URL}/tokens/${OKTA.id}`;

const MAPPING_URL = `${SCIM_URL}/groups/${ENGINEERING.id}/mapping`;

function respond(body: unknown, status = 200) {
  return fetchSpy.mockImplementation(async () => Response.json(body, { status }));
}

function empty(status: number) {
  return fetchSpy.mockImplementation(async () => new Response(null, { status }));
}

function sent(request: ReturnType<typeof respond>) {
  const [url, init] = request.mock.calls[0];
  return {
    url,
    method: init?.method,
    body: init?.body ? JSON.parse(String(init.body)) : undefined,
  };
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

describe('scimApi.settings', () => {
  it('reads the SCIM settings of the organization with the session token', async () => {
    session.setAccessToken('token-1');
    const request = respond(ACTIVE);

    expect(await scimApi.settings(ORGANIZATION_ID)).toEqual(ACTIVE);
    expect(sent(request)).toEqual({ url: SCIM_URL, method: 'GET', body: undefined });
    expect(new Headers(request.mock.calls[0][1]?.headers).get('Authorization')).toBe(
      'Bearer token-1'
    );
  });

  it('escapes the organization id in the path', async () => {
    const request = respond(ACTIVE);

    await scimApi.settings('a/b');

    expect(sent(request).url).toBe('/api/organizations/a%2Fb/scim');
  });

  it('rejects a group mapped to the owner role', async () => {
    respond({ ...ACTIVE, groups: [{ ...ENGINEERING, role: 'owner' }] });

    expect(await failure(scimApi.settings(ORGANIZATION_ID))).toBeInstanceOf(Error);
  });

  it('throws a plain ApiError for a refusal without a SCIM code', async () => {
    respond({ error: 'Forbidden' }, 403);

    const error = await failure(scimApi.settings(ORGANIZATION_ID));

    expect(error).toBeInstanceOf(ApiError);
    expect(error).not.toBeInstanceOf(ScimRefusal);
    expect((error as ApiError).message).toBe('Failed to load SCIM provisioning: Forbidden');
    expect((error as ApiError).status).toBe(403);
  });
});

describe('scimApi tokens', () => {
  it('creates a named token and returns its secret', async () => {
    const request = respond(ISSUED, 201);

    expect(await scimApi.createToken(ORGANIZATION_ID, 'Google Workspace')).toEqual(ISSUED);
    expect(sent(request)).toEqual({
      url: `${SCIM_URL}/tokens`,
      method: 'POST',
      body: { name: 'Google Workspace' },
    });
  });

  it('rejects an issued token without a secret', async () => {
    respond({ token: ISSUED.token, secret: '' }, 201);

    expect(await failure(scimApi.createToken(ORGANIZATION_ID, 'Okta'))).toBeInstanceOf(Error);
  });

  it('rotates a token through its rotations collection', async () => {
    const request = respond(ROTATED, 201);

    expect(await scimApi.rotateToken(ORGANIZATION_ID, OKTA.id)).toEqual(ROTATED);
    expect(sent(request)).toEqual({
      url: `${TOKEN_URL}/rotations`,
      method: 'POST',
      body: undefined,
    });
  });

  it('revokes a token with a DELETE', async () => {
    const request = empty(204);

    await scimApi.revokeToken(ORGANIZATION_ID, OKTA.id);

    expect(sent(request)).toEqual({ url: TOKEN_URL, method: 'DELETE', body: undefined });
  });

  it('raises a SCIM refusal for a token that is gone', async () => {
    respond({ error: 'SCIM token not found', code: ScimCode.TokenNotFound }, 404);

    const error = await failure(scimApi.revokeToken(ORGANIZATION_ID, OKTA.id));

    expect(error).toBeInstanceOf(ScimRefusal);
    expect((error as ScimRefusal).code).toBe(ScimCode.TokenNotFound);
    expect((error as ScimRefusal).status).toBe(404);
  });

  it('raises a SCIM refusal for a lapsed license', async () => {
    respond({ error: 'Enterprise', code: ScimCode.LicenseRequired }, 403);

    const error = await failure(scimApi.rotateToken(ORGANIZATION_ID, OKTA.id));

    expect((error as ScimRefusal).code).toBe(ScimCode.LicenseRequired);
  });
});

describe('scimApi group mappings', () => {
  it('maps a group to a built-in role', async () => {
    const saved = { ...ENGINEERING, role: MappedRole.Admin };
    const request = respond(saved);

    expect(
      await scimApi.mapGroup(ORGANIZATION_ID, ENGINEERING.id, { role: MappedRole.Admin })
    ).toEqual(saved);
    expect(sent(request)).toEqual({
      url: MAPPING_URL,
      method: 'PUT',
      body: { role: MappedRole.Admin },
    });
  });

  it('maps a group to a custom role', async () => {
    const saved = { ...ENGINEERING, role: null, custom_role: { id: AUDITOR.id, name: 'Auditor' } };
    const request = respond(saved);

    await scimApi.mapGroup(ORGANIZATION_ID, ENGINEERING.id, { custom_role_id: AUDITOR.id });

    expect(sent(request).body).toEqual({ custom_role_id: AUDITOR.id });
  });

  it('unmaps a group with a DELETE of its mapping', async () => {
    const saved = { ...ENGINEERING, role: null };
    const request = respond(saved);

    expect(await scimApi.unmapGroup(ORGANIZATION_ID, ENGINEERING.id)).toEqual(saved);
    expect(sent(request)).toEqual({ url: MAPPING_URL, method: 'DELETE', body: undefined });
  });

  it('raises a SCIM refusal for a mapping past the caller', async () => {
    respond({ error: 'Exceeds', code: ScimCode.Exceeds }, 403);

    const error = await failure(
      scimApi.mapGroup(ORGANIZATION_ID, ENGINEERING.id, { custom_role_id: AUDITOR.id })
    );

    expect((error as ScimRefusal).code).toBe(ScimCode.Exceeds);
  });
});
