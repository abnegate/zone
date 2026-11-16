import { beforeEach, describe, expect, it, mock } from 'bun:test';
import { NO_SSO, OKTA, SSO_OFFERED } from '../features/auth/ssoFixtures';
import { AuthRequestError } from './auth';
import { authorize, completeCallback, listProviders } from './sso';

const mockFetch = mock();
global.fetch = mockFetch as typeof fetch;

const AUTH_RESPONSE = {
  access_token: 'access',
  refresh_token: 'refresh',
  expires_in: 3600,
  user: {
    id: '1',
    email: 'ada@acme.com',
    email_verified: true,
    display_name: 'Ada',
    is_active: true,
    is_admin: false,
    created_at: '2026-01-01T00:00:00Z',
    updated_at: '2026-01-01T00:00:00Z',
    last_login_at: null,
  },
  roles: ['user'],
  permissions: ['chats:read'],
};

function respond(status: number, body: unknown) {
  mockFetch.mockResolvedValueOnce({
    ok: status >= 200 && status < 300,
    status,
    json: () => Promise.resolve(body),
  });
}

describe('sso API', () => {
  beforeEach(() => {
    mockFetch.mockReset();
  });

  describe('listProviders', () => {
    it('gets the public providers', async () => {
      respond(200, SSO_OFFERED);

      expect(await listProviders()).toEqual(SSO_OFFERED);
      expect(mockFetch).toHaveBeenCalledWith('/api/auth/sso/providers', expect.anything());
    });

    it('reads an empty list', async () => {
      respond(200, NO_SSO);

      expect(await listProviders()).toEqual(NO_SSO);
    });

    it('refuses a body without the contract fields', async () => {
      respond(200, {});

      await expect(listProviders()).rejects.toThrow('Validation failed');
    });

    it('refuses a body without callback_origin', async () => {
      respond(200, { required: false, providers: [] });

      await expect(listProviders()).rejects.toThrow('callback_origin');
    });

    it('throws when the server fails', async () => {
      respond(500, { error: 'boom' });

      await expect(listProviders()).rejects.toThrow('boom');
    });
  });

  describe('authorize', () => {
    it('posts an empty body to the provider authorizations and returns the URL and state', async () => {
      const authorization = { url: 'https://acme.okta.com/authorize?state=s1', state: 's1' };
      respond(201, authorization);

      expect(await authorize(OKTA.id)).toEqual(authorization);
      expect(mockFetch).toHaveBeenCalledWith(
        `/api/auth/sso/providers/${OKTA.id}/authorizations`,
        expect.objectContaining({ method: 'POST', body: '{}' })
      );
    });

    it('encodes the provider id into the path', async () => {
      respond(201, { url: 'https://idp.test/authorize', state: 's1' });

      await authorize('a/b?c');

      expect(mockFetch.mock.calls[0][0]).toBe('/api/auth/sso/providers/a%2Fb%3Fc/authorizations');
    });

    it.each(['javascript:alert(1)', 'data:text/html,hi', '//idp.test/authorize'])(
      'refuses to hand back a %s URL',
      async (url) => {
        respond(201, { url, state: 's1' });

        await expect(authorize(OKTA.id)).rejects.toThrow('Validation failed');
      }
    );

    it('throws the refusal code of an unknown provider', async () => {
      respond(404, { error: 'Provider not found', code: 'sso_provider_not_found' });

      await expect(authorize(OKTA.id)).rejects.toMatchObject({
        name: 'AuthRequestError',
        code: 'sso_provider_not_found',
      });
    });
  });

  describe('completeCallback', () => {
    it('posts the state and code with the device and time-zone headers', async () => {
      respond(200, AUTH_RESPONSE);

      expect(await completeCallback({ state: 's1', code: 'c1' })).toEqual(AUTH_RESPONSE);
      expect(mockFetch).toHaveBeenCalledWith(
        '/api/auth/sso/callbacks',
        expect.objectContaining({
          method: 'POST',
          headers: expect.objectContaining({
            'Content-Type': 'application/json',
            'X-Zone-Device': expect.any(String),
            'X-Zone-Device-Platform': 'browser',
            'X-Zone-Time-Zone': Intl.DateTimeFormat().resolvedOptions().timeZone,
          }),
          body: JSON.stringify({ state: 's1', code: 'c1' }),
        })
      );
    });

    it.each([
      [400, 'sso_state_invalid'],
      [401, 'sso_id_token_invalid'],
      [403, 'sso_domain_not_allowed'],
      [403, 'account_disabled'],
      [409, 'sso_email_conflict'],
      [502, 'sso_token_exchange_failed'],
    ])('throws a %i %s refusal with its code', async (status, code) => {
      respond(status, { error: 'Refused', code });

      const failure = await completeCallback({ state: 's1', code: 'c1' }).catch(
        (error: unknown) => error
      );
      expect(failure).toBeInstanceOf(AuthRequestError);
      expect((failure as AuthRequestError).code).toBe(code);
    });

    it('falls back to a generic message when the refusal is not JSON', async () => {
      mockFetch.mockResolvedValueOnce({
        ok: false,
        status: 502,
        json: () => Promise.reject(new Error('not json')),
      });

      await expect(completeCallback({ state: 's1', code: 'c1' })).rejects.toThrow(
        'Single sign-on failed'
      );
    });
  });
});
