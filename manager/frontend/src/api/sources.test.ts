import { afterEach, describe, expect, it, mock } from 'bun:test';
import { sourcesApi } from './sources';

const fetch = globalThis.fetch;

afterEach(() => {
  globalThis.fetch = fetch;
});

describe('sourcesApi.verifySource', () => {
  for (const verified of [true, false]) {
    it(`parses the backend response with verified=${verified}`, async () => {
      const response = {
        verified,
        message: verified ? 'Source verified successfully' : 'Authentication failed',
      };
      const request = mock(async () => new Response(JSON.stringify(response)));
      globalThis.fetch = request as unknown as typeof globalThis.fetch;

      expect(await sourcesApi.verifySource('workspace', 'source')).toEqual(response);
      expect(request).toHaveBeenCalledWith(
        expect.stringContaining('/api/workspaces/workspace/sources/source/verify'),
        expect.objectContaining({ method: 'POST' })
      );
    });
  }

  for (const response of [
    { message: 'Missing result' },
    { success: true, message: 'Obsolete result' },
    { verified: 'true', message: 'Wrong type' },
    { verified: null, message: 'Null result' },
  ]) {
    it(`rejects malformed verification: ${JSON.stringify(response)}`, async () => {
      globalThis.fetch = mock(
        async () => new Response(JSON.stringify(response))
      ) as unknown as typeof globalThis.fetch;

      await expect(sourcesApi.verifySource('workspace', 'source')).rejects.toThrow();
    });
  }
});
