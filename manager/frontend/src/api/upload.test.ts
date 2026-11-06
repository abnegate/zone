import { afterEach, beforeEach, describe, expect, it } from 'bun:test';
import { installFakeXhr } from '../test/xhr';
import { isCancelled, upload } from './upload';

describe('upload', () => {
  let xhr: ReturnType<typeof installFakeXhr>;

  beforeEach(() => {
    xhr = installFakeXhr();
  });

  afterEach(() => {
    xhr.restore();
  });

  it('sends the body with the given headers and reports each progress event', async () => {
    const progress: Array<[number, number]> = [];
    const body = new FormData();
    body.append('file', new Blob(['bytes']), 'a.bin');
    const response = upload('/api/files', {
      headers: { Authorization: 'Bearer token' },
      body,
      onProgress: ({ loaded, total }) => progress.push([loaded, total]),
    });

    const [request] = xhr.requests;
    expect(request.method).toBe('POST');
    expect(request.url).toBe('/api/files');
    expect(request.headers.authorization).toBe('Bearer token');
    expect(request.body).toBe(body);

    request.progress(1, 4);
    request.progress(4, 4);
    request.respond(201, { id: 'file-1' }, { 'x-request-id': 'abc' });

    const result = await response;
    expect(progress).toEqual([
      [1, 4],
      [4, 4],
    ]);
    expect(result.status).toBe(201);
    expect(result.ok).toBe(true);
    expect(result.headers.get('x-request-id')).toBe('abc');
    expect(await result.json()).toEqual({ id: 'file-1' });
  });

  it('resolves an error status as a response so callers keep their own error handling', async () => {
    const response = upload('/api/files', { body: 'x' });
    xhr.requests[0].respond(413, { error: 'too large' });

    const result = await response;
    expect(result.ok).toBe(false);
    expect(result.status).toBe(413);
    expect(await result.json()).toEqual({ error: 'too large' });
  });

  it('rejects when the server cannot be reached', async () => {
    const response = upload('/api/files', { body: 'x' });
    xhr.requests[0].fail();

    await expect(response).rejects.toThrow('The upload could not reach the server');
  });

  it('aborts an upload in flight and rejects as cancelled', async () => {
    const controller = new AbortController();
    const response = upload('/api/files', { body: 'x', signal: controller.signal });
    controller.abort();

    const error = await response.catch((reason: unknown) => reason);
    expect(isCancelled(error)).toBe(true);
    expect(xhr.requests[0].aborted).toBe(true);
  });

  it('never opens a request for a signal that is already aborted', async () => {
    const controller = new AbortController();
    controller.abort();

    const error = await upload('/api/files', { body: 'x', signal: controller.signal }).catch(
      (reason: unknown) => reason
    );
    expect(isCancelled(error)).toBe(true);
    expect(xhr.requests).toHaveLength(0);
  });
});
