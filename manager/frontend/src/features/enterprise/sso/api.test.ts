import { afterAll, afterEach, describe, expect, it, spyOn } from 'bun:test';
import { ApiError } from '../../../api/ApiError';
import { session } from '../../../api/session';
import { ssoApi } from './api';
import { ENTITLED, OKTA } from './fixtures';
import { SsoRefusal } from './SsoRefusal';
import { SSO_ADMIN_CODES, SsoAdminCode } from './schemas';

const fetchSpy = spyOn(globalThis, 'fetch');

function respond(body: unknown, status = 200) {
  return fetchSpy.mockImplementation(async () => Response.json(body, { status }));
}

function respondEmpty(status: number) {
  return fetchSpy.mockImplementation(async () => new Response(null, { status }));
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

const CREATE = {
  display_name: 'Okta',
  issuer: 'https://acme.okta.com',
  client_id: '0oa1b2c3d4',
  client_secret: 's3cret',
  allowed_domains: ['acme.com'],
  require_verified_email: true,
  enabled: true,
};

afterEach(() => {
  fetchSpy.mockReset();
  session.setAccessToken(null);
});

afterAll(() => {
  fetchSpy.mockRestore();
});

describe('ssoApi.get', () => {
  it('reads the single sign-on settings of this server', async () => {
    const request = respond(ENTITLED);

    expect(await ssoApi.get()).toEqual(ENTITLED);
    expect(sent(request)).toEqual({ url: '/api/sso', method: 'GET', body: undefined });
  });

  it('sends the session token', async () => {
    session.setAccessToken('token-1');
    const request = respond(ENTITLED);

    await ssoApi.get();

    const headers = new Headers(request.mock.calls[0][1]?.headers);
    expect(headers.get('Authorization')).toBe('Bearer token-1');
  });

  it('rejects a response outside the contract', async () => {
    respond({ ...ENTITLED, entitled: 'yes' });

    expect(await failure(ssoApi.get())).toBeInstanceOf(Error);
  });

  it('throws an ApiError carrying the status when the user is not a server admin', async () => {
    respond({ error: 'This needs a server admin.' }, 403);

    const error = await failure(ssoApi.get());

    expect(error).toBeInstanceOf(ApiError);
    expect(error).not.toBeInstanceOf(SsoRefusal);
    expect((error as ApiError).status).toBe(403);
    expect((error as ApiError).message).toBe(
      'Failed to load single sign-on: This needs a server admin.'
    );
  });
});

describe('ssoApi.createProvider', () => {
  it('posts the provider and reads it back', async () => {
    const request = respond(OKTA, 201);

    expect(await ssoApi.createProvider(CREATE)).toEqual(OKTA);
    expect(sent(request)).toEqual({
      url: '/api/sso/providers',
      method: 'POST',
      body: JSON.stringify(CREATE),
    });
  });

  it('sends nothing without a client secret', async () => {
    const request = respond(OKTA, 201);

    expect(await failure(ssoApi.createProvider({ ...CREATE, client_secret: '' }))).toBeInstanceOf(
      Error
    );
    expect(request).not.toHaveBeenCalled();
  });

  for (const [code, status] of [
    [SsoAdminCode.LicenseRequired, 403],
    [SsoAdminCode.IssuerInvalid, 400],
    [SsoAdminCode.DiscoveryFailed, 400],
    [SsoAdminCode.ProviderDuplicate, 409],
  ] as const) {
    it(`throws an SsoRefusal carrying ${code} and its ${status}`, async () => {
      respond({ error: 'Refused by the server', code }, status);

      const error = await failure(ssoApi.createProvider(CREATE));

      expect(error).toBeInstanceOf(SsoRefusal);
      expect((error as SsoRefusal).code).toBe(code);
      expect((error as SsoRefusal).status).toBe(status);
      expect((error as SsoRefusal).message).toBe('Refused by the server');
    });
  }

  it('throws a plain ApiError for a code outside the contract', async () => {
    respond({ error: 'Something new', code: 'sso_unheard_of' }, 400);

    const error = await failure(ssoApi.createProvider(CREATE));

    expect(error).not.toBeInstanceOf(SsoRefusal);
    expect((error as ApiError).message).toBe('Failed to add the provider: Something new');
  });

  it('throws a plain ApiError for a body that is not JSON', async () => {
    fetchSpy.mockImplementation(async () => new Response('Bad gateway', { status: 502 }));

    const error = await failure(ssoApi.createProvider(CREATE));

    expect(error).toBeInstanceOf(ApiError);
    expect(error).not.toBeInstanceOf(SsoRefusal);
    expect((error as ApiError).message).toBe('Failed to add the provider: 502');
  });
});

describe('ssoApi.updateProvider', () => {
  it('patches only the fields it is given', async () => {
    const request = respond({ ...OKTA, enabled: false });

    expect(await ssoApi.updateProvider(OKTA.id, { enabled: false })).toEqual({
      ...OKTA,
      enabled: false,
    });
    expect(sent(request)).toEqual({
      url: `/api/sso/providers/${OKTA.id}`,
      method: 'PATCH',
      body: JSON.stringify({ enabled: false }),
    });
  });

  it('escapes the provider id in the path', async () => {
    const request = respond(OKTA);

    await ssoApi.updateProvider('a/b?c', { enabled: true });

    expect(sent(request).url).toBe('/api/sso/providers/a%2Fb%3Fc');
  });

  for (const [code, status] of [
    [SsoAdminCode.ProviderNotFound, 404],
    [SsoAdminCode.ProviderRequired, 409],
  ] as const) {
    it(`throws an SsoRefusal carrying ${code}`, async () => {
      respond({ error: 'Refused', code }, status);

      const error = await failure(ssoApi.updateProvider(OKTA.id, { enabled: false }));

      expect((error as SsoRefusal).code).toBe(code);
      expect((error as SsoRefusal).status).toBe(status);
    });
  }
});

describe('ssoApi.deleteProvider', () => {
  it('deletes the provider and accepts an empty 204', async () => {
    const request = respondEmpty(204);

    expect(await ssoApi.deleteProvider(OKTA.id)).toBeUndefined();
    expect(sent(request)).toEqual({
      url: `/api/sso/providers/${OKTA.id}`,
      method: 'DELETE',
      body: undefined,
    });
  });

  it('throws an SsoRefusal when the last enabled provider is still required', async () => {
    respond({ error: 'Refused', code: SsoAdminCode.ProviderRequired }, 409);

    const error = await failure(ssoApi.deleteProvider(OKTA.id));

    expect((error as SsoRefusal).code).toBe(SsoAdminCode.ProviderRequired);
  });
});

describe('ssoApi.setEnforcement', () => {
  for (const required of [true, false]) {
    it(`puts required ${required}`, async () => {
      const request = respond({ required });

      expect(await ssoApi.setEnforcement(required)).toEqual({ required });
      expect(sent(request)).toEqual({
        url: '/api/sso/enforcement',
        method: 'PUT',
        body: JSON.stringify({ required }),
      });
    });
  }

  it('throws an SsoRefusal when no provider is enabled', async () => {
    respond({ error: 'Refused', code: SsoAdminCode.ProviderRequired }, 409);

    expect(((await failure(ssoApi.setEnforcement(true))) as SsoRefusal).code).toBe(
      SsoAdminCode.ProviderRequired
    );
  });

  it('rejects an enforcement response outside the contract', async () => {
    respond({ required: 'yes' });

    expect(await failure(ssoApi.setEnforcement(true))).toBeInstanceOf(Error);
  });
});

describe('refusal coverage', () => {
  for (const code of SSO_ADMIN_CODES) {
    it(`recognizes ${code} on any write`, async () => {
      respond({ error: 'Refused', code }, 400);

      expect(await failure(ssoApi.setEnforcement(false))).toBeInstanceOf(SsoRefusal);
    });
  }
});
