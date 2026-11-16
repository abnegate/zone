import { afterAll, afterEach, describe, expect, it, spyOn } from 'bun:test';
import { ApiError } from '../../api/ApiError';
import { session } from '../../api/session';
import { licenseApi } from './api';
import { ACTIVE, COMMUNITY } from './fixtures';
import { LicenseRefusal } from './LicenseRefusal';
import { LICENSE_REFUSAL_CODES } from './schemas';

const fetchSpy = spyOn(globalThis, 'fetch');

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

describe('licenseApi.get', () => {
  it('reads the license of this server', async () => {
    const request = respond(ACTIVE);

    expect(await licenseApi.get()).toEqual(ACTIVE);
    expect(sent(request)).toEqual({ url: '/api/license', method: 'GET', body: undefined });
  });

  it('sends the session token', async () => {
    session.setAccessToken('token-1');
    const request = respond(COMMUNITY);

    await licenseApi.get();

    const headers = new Headers(request.mock.calls[0][1]?.headers);
    expect(headers.get('Authorization')).toBe('Bearer token-1');
  });

  it('rejects a response outside the contract', async () => {
    respond({ ...ACTIVE, status: 'expired' });

    await expect(licenseApi.get()).rejects.toThrow('Validation failed');
  });

  it('throws an ApiError carrying the status when the user is not a server admin', async () => {
    respond({ error: 'This needs a server admin.' }, 403);

    const error = await failure(licenseApi.get());

    expect(error).toBeInstanceOf(ApiError);
    expect(error).not.toBeInstanceOf(LicenseRefusal);
    expect((error as ApiError).status).toBe(403);
    expect((error as ApiError).message).toBe(
      'Failed to load the license: This needs a server admin.'
    );
  });
});

describe('licenseApi.set', () => {
  it('puts the key and reads the new license', async () => {
    const request = respond(ACTIVE);

    expect(await licenseApi.set('payload.signature')).toEqual(ACTIVE);
    expect(sent(request)).toEqual({
      url: '/api/license',
      method: 'PUT',
      body: JSON.stringify({ key: 'payload.signature' }),
    });
  });

  it('keeps line breaks inside the key for the server to strip', async () => {
    const request = respond(ACTIVE);

    await licenseApi.set('  payload\n.signature\n');

    expect(sent(request).body).toBe(JSON.stringify({ key: 'payload\n.signature' }));
  });

  for (const code of LICENSE_REFUSAL_CODES) {
    it(`throws a LicenseRefusal carrying ${code}`, async () => {
      respond({ error: 'Refused', code }, 400);

      const error = await failure(licenseApi.set('payload.signature'));

      expect(error).toBeInstanceOf(LicenseRefusal);
      expect((error as LicenseRefusal).code).toBe(code);
      expect((error as LicenseRefusal).status).toBe(400);
      expect((error as LicenseRefusal).message).toBe('Refused');
    });
  }

  it('throws a plain ApiError for a 400 without a refusal code', async () => {
    respond({ error: 'Invalid JSON body' }, 400);

    const error = await failure(licenseApi.set('payload.signature'));

    expect(error).toBeInstanceOf(ApiError);
    expect(error).not.toBeInstanceOf(LicenseRefusal);
    expect((error as ApiError).message).toBe('Failed to save the license: Invalid JSON body');
  });

  it('throws a plain ApiError for a 400 with a code it does not know', async () => {
    respond({ error: 'Something new', code: 'license_unheard_of' }, 400);

    const error = await failure(licenseApi.set('payload.signature'));

    expect(error).not.toBeInstanceOf(LicenseRefusal);
    expect((error as ApiError).message).toBe('Failed to save the license: Something new');
  });

  it('throws an ApiError when the user is not a server admin', async () => {
    respond({ error: 'This needs a server admin.' }, 403);

    const error = await failure(licenseApi.set('payload.signature'));

    expect(error).toBeInstanceOf(ApiError);
    expect(error).not.toBeInstanceOf(LicenseRefusal);
    expect((error as ApiError).status).toBe(403);
  });

  it('sends nothing for a key that is only whitespace', async () => {
    const request = respond(ACTIVE);

    await expect(licenseApi.set('  \n')).rejects.toThrow('Validation failed');
    expect(request).not.toHaveBeenCalled();
  });
});

describe('licenseApi.remove', () => {
  it('deletes the license and reads the Community response', async () => {
    const request = respond(COMMUNITY);

    expect(await licenseApi.remove()).toEqual(COMMUNITY);
    expect(sent(request)).toEqual({ url: '/api/license', method: 'DELETE', body: undefined });
  });

  it('throws an ApiError when the user is not a server admin', async () => {
    respond({ error: 'This needs a server admin.' }, 403);

    const error = await failure(licenseApi.remove());

    expect(error).toBeInstanceOf(ApiError);
    expect((error as ApiError).status).toBe(403);
  });
});
