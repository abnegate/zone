import { afterEach, beforeEach, describe, expect, it, mock } from 'bun:test';
import {
  fetchProtectedImage,
  fetchSignedArtifactUrl,
  isProtectedArtifactUrl,
  isProtectedImageUrl,
} from './protectedImages';

const ARTIFACT_URL = '/api/artifacts/artifact-1/content';
const PREVIEW_URL = '/api/models/train/previews/step-1500-0.png';
const SESSION_TOKEN = 'session-token';

const originalFetch = globalThis.fetch;
const fetchMock = mock();

beforeEach(() => {
  fetchMock.mockReset();
  globalThis.fetch = fetchMock as unknown as typeof fetch;
});

afterEach(() => {
  globalThis.fetch = originalFetch;
});

describe('isProtectedImageUrl', () => {
  it('accepts artifact and training preview urls', () => {
    expect(isProtectedImageUrl(ARTIFACT_URL)).toBe(true);
    expect(isProtectedImageUrl(PREVIEW_URL)).toBe(true);
  });

  it('rejects urls outside the protected prefixes', () => {
    for (const url of [
      '/assets/logo.png',
      '/api/models/train/previews',
      '/api/models/train/status',
      '/api/artifacts',
      `https://example.test${PREVIEW_URL}`,
      `https://example.test${ARTIFACT_URL}`,
      'data:image/png;base64,AAAA',
      `blob:${PREVIEW_URL}`,
    ]) {
      expect(isProtectedImageUrl(url)).toBe(false);
    }
  });

  it('keeps training previews out of the artifact check', () => {
    expect(isProtectedArtifactUrl(ARTIFACT_URL)).toBe(true);
    expect(isProtectedArtifactUrl(PREVIEW_URL)).toBe(false);
  });
});

describe('fetchProtectedImage', () => {
  it('fetches a training preview with the bearer token', async () => {
    const previewBlob = new Blob(['preview'], { type: 'image/png' });
    fetchMock.mockResolvedValue({
      ok: true,
      status: 200,
      blob: async () => previewBlob,
    } as Response);
    const controller = new AbortController();

    await expect(fetchProtectedImage(PREVIEW_URL, controller.signal, SESSION_TOKEN)).resolves.toBe(
      previewBlob
    );
    expect(fetchMock).toHaveBeenCalledTimes(1);
    expect(fetchMock).toHaveBeenCalledWith(PREVIEW_URL, {
      headers: { Authorization: `Bearer ${SESSION_TOKEN}` },
      signal: controller.signal,
    });
  });

  it('refuses to send the token to an unprotected url', async () => {
    await expect(fetchProtectedImage('/assets/logo.png', undefined, SESSION_TOKEN)).rejects.toThrow(
      'Only protected images can be fetched with authentication'
    );
    await expect(
      fetchProtectedImage(`https://example.test${PREVIEW_URL}`, undefined, SESSION_TOKEN)
    ).rejects.toThrow('Only protected images can be fetched with authentication');
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it('refuses to fetch a training preview without a session', async () => {
    await expect(fetchProtectedImage(PREVIEW_URL, undefined, null)).rejects.toThrow(
      'Authentication is required to load this image'
    );
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it('rejects a training preview the server refuses', async () => {
    fetchMock.mockResolvedValue({ ok: false, status: 401 } as Response);

    await expect(fetchProtectedImage(PREVIEW_URL, undefined, SESSION_TOKEN)).rejects.toThrow(
      'Failed to load image: 401'
    );
  });
});

describe('fetchSignedArtifactUrl', () => {
  it('never signs a training preview', async () => {
    await expect(fetchSignedArtifactUrl(PREVIEW_URL, undefined, SESSION_TOKEN)).rejects.toThrow(
      'Only protected artifacts can be signed'
    );
    expect(fetchMock).not.toHaveBeenCalled();
  });
});
