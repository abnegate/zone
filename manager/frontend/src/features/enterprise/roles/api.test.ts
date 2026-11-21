import { afterAll, afterEach, describe, expect, it, spyOn } from 'bun:test';
import { ApiError } from '../../../api/ApiError';
import { session } from '../../../api/session';
import { rolesApi } from './api';
import { ACTIVE, AUDITOR, ORGANIZATION_ID, RELEASE_MANAGER } from './fixtures';
import { RoleRefusal } from './RoleRefusal';
import { Permission, RoleCode, RoleScope } from './schemas';

const fetchSpy = spyOn(globalThis, 'fetch');

const ROLES_URL = `/api/organizations/${ORGANIZATION_ID}/roles`;

const ROLE_URL = `${ROLES_URL}/${RELEASE_MANAGER.id}`;

function respond(body: unknown, status = 200) {
  return fetchSpy.mockImplementation(async () => Response.json(body, { status }));
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

describe('rolesApi.list', () => {
  it('reads the roles of the organization with the session token', async () => {
    session.setAccessToken('token-1');
    const request = respond(ACTIVE);

    expect(await rolesApi.list(ORGANIZATION_ID)).toEqual(ACTIVE);
    expect(sent(request)).toEqual({ url: ROLES_URL, method: 'GET', body: undefined });
    expect(new Headers(request.mock.calls[0][1]?.headers).get('Authorization')).toBe(
      'Bearer token-1'
    );
  });

  it('escapes the organization id in the path', async () => {
    const request = respond(ACTIVE);

    await rolesApi.list('a/b');

    expect(sent(request).url).toBe('/api/organizations/a%2Fb/roles');
  });

  it('rejects a response outside the contract', async () => {
    respond({
      ...ACTIVE,
      catalogue: [{ name: 'nothing:here', scope: 'server', composable: true }],
    });

    expect(await failure(rolesApi.list(ORGANIZATION_ID))).toBeInstanceOf(Error);
  });

  it('throws a plain ApiError for a failure without a role code', async () => {
    respond({ error: 'Organization not found' }, 404);

    const error = await failure(rolesApi.list(ORGANIZATION_ID));

    expect(error).toBeInstanceOf(ApiError);
    expect(error).not.toBeInstanceOf(RoleRefusal);
    expect((error as ApiError).message).toBe('Failed to load the roles: Organization not found');
  });
});

describe('rolesApi.create', () => {
  it('posts the definition and reads back the role', async () => {
    const request = respond({ role: AUDITOR }, 201);

    const role = await rolesApi.create(ORGANIZATION_ID, {
      name: 'Auditor',
      description: null,
      scope: RoleScope.Organization,
      permissions: [Permission.AuditLogsRead],
    });

    expect(role).toEqual(AUDITOR);
    expect(sent(request)).toEqual({
      url: ROLES_URL,
      method: 'POST',
      body: {
        name: 'Auditor',
        description: null,
        scope: RoleScope.Organization,
        permissions: [Permission.AuditLogsRead],
      },
    });
  });

  it('refuses to send a field the server does not read', async () => {
    const request = respond({ role: AUDITOR }, 201);

    const error = await failure(
      rolesApi.create(ORGANIZATION_ID, {
        name: 'Auditor',
        scope: RoleScope.Organization,
        permissions: [],
        members: 1,
      } as never)
    );

    expect(error).toBeInstanceOf(Error);
    expect(request).not.toHaveBeenCalled();
  });

  it('turns a taken name into a RoleRefusal that keeps its status', async () => {
    respond({ error: 'A role with this name exists', code: RoleCode.NameTaken }, 409);

    const error = await failure(
      rolesApi.create(ORGANIZATION_ID, {
        name: 'Auditor',
        scope: RoleScope.Organization,
        permissions: [],
      })
    );

    expect(error).toBeInstanceOf(RoleRefusal);
    expect((error as RoleRefusal).code).toBe(RoleCode.NameTaken);
    expect((error as RoleRefusal).status).toBe(409);
    expect((error as RoleRefusal).members).toBeUndefined();
  });
});

describe('rolesApi.update', () => {
  it('patches only the keys it is given', async () => {
    const request = respond({ role: RELEASE_MANAGER });

    expect(
      await rolesApi.update(ORGANIZATION_ID, RELEASE_MANAGER.id, { description: null })
    ).toEqual(RELEASE_MANAGER);
    expect(sent(request)).toEqual({ url: ROLE_URL, method: 'PATCH', body: { description: null } });
  });

  it('refuses to change the scope', async () => {
    const request = respond({ role: RELEASE_MANAGER });

    const error = await failure(
      rolesApi.update(ORGANIZATION_ID, RELEASE_MANAGER.id, {
        scope: RoleScope.Organization,
      } as never)
    );

    expect(error).toBeInstanceOf(Error);
    expect(request).not.toHaveBeenCalled();
  });

  it('turns a widening refusal into a RoleRefusal', async () => {
    respond({ error: 'Exceeds', code: RoleCode.Exceeds }, 403);

    const error = await failure(
      rolesApi.update(ORGANIZATION_ID, RELEASE_MANAGER.id, {
        permissions: [Permission.WorkspaceUpdate],
      })
    );

    expect((error as RoleRefusal).code).toBe(RoleCode.Exceeds);
    expect((error as RoleRefusal).status).toBe(403);
  });
});

describe('rolesApi.remove', () => {
  it('deletes the role', async () => {
    const request = fetchSpy.mockImplementation(async () => new Response(null, { status: 204 }));

    expect(await rolesApi.remove(ORGANIZATION_ID, RELEASE_MANAGER.id)).toBeUndefined();
    expect(sent(request)).toEqual({ url: ROLE_URL, method: 'DELETE', body: undefined });
  });

  it('reads how many members still hold a role in use', async () => {
    respond({ error: 'Role in use', code: RoleCode.InUse, members: 3 }, 409);

    const error = await failure(rolesApi.remove(ORGANIZATION_ID, RELEASE_MANAGER.id));

    expect(error).toBeInstanceOf(RoleRefusal);
    expect((error as RoleRefusal).code).toBe(RoleCode.InUse);
    expect((error as RoleRefusal).members).toBe(3);
  });

  it('throws a plain ApiError for a missing role', async () => {
    respond({ error: 'Role not found' }, 404);

    const error = await failure(rolesApi.remove(ORGANIZATION_ID, RELEASE_MANAGER.id));

    expect(error).not.toBeInstanceOf(RoleRefusal);
    expect((error as ApiError).message).toBe('Failed to delete the role: Role not found');
  });
});
