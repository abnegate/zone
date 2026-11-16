import { describe, expect, it } from 'bun:test';
import { AuthorizedApi } from './AuthorizedApi';

class Probe extends AuthorizedApi {
  sent(): Record<string, string> {
    return this.headers() as Record<string, string>;
  }
}

describe('AuthorizedApi', () => {
  it('sends JSON with the device headers and the bearer token', () => {
    const api = new Probe();
    api.setGetAccessToken(() => 'token-1');

    const headers = api.sent();

    expect(headers['Content-Type']).toBe('application/json');
    expect(headers.Authorization).toBe('Bearer token-1');
    expect(headers['X-Zone-Device']).toBeTruthy();
  });

  it('sends no authorization without a session', () => {
    const api = new Probe();
    api.setGetAccessToken(() => null);

    expect(api.sent()).not.toHaveProperty('Authorization');
  });
});
