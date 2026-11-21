import { beforeAll, describe, expect, test } from 'bun:test';
import { build, defaultBaseURL } from './build.ts';
import { respond } from './serve.ts';

const origin = 'http://127.0.0.1:4321';

describe('preview server', () => {
  beforeAll(() => build(defaultBaseURL()));

  test('redirects a directory to its trailing slash', () => {
    const response = respond(new Request(`${origin}/pricing`));
    expect(response.status).toBe(301);
    expect(response.headers.get('location')).toBe(`${origin}/pricing/`);
  });

  test('keeps a redirect on its own origin when the path encodes another host', () => {
    const response = respond(new Request(`${origin}/%2fevil.example%2f..%2fpricing`));
    expect(response.status).toBe(301);
    const location = new URL(response.headers.get('location') ?? '', `${origin}/`);
    expect(location.origin).toBe(origin);
  });
});
