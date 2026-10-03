import { afterEach, describe, expect, it, mock } from 'bun:test';
import { ApiError } from './ApiError';
import { modelsApi } from './models';

describe('Namespaced model requests', () => {
  const original = global.fetch;
  const name = 'hf.co/owner/repository:Q4_K_M';

  it('preserves installed model capabilities through response validation', async () => {
    const model = {
      name,
      size: 123,
      modified_at: '2024-01-01T00:00:00Z',
      capabilities: ['text', 'image_input', 'audio', 'video_input', 'tools'],
    };
    global.fetch = mock(async () => Response.json([model])) as typeof fetch;

    expect(await modelsApi.getModels()).toEqual({ models: [model] });
  });

  it('lists every installed model when no workspace is named', async () => {
    const request = mock(async () => Response.json([]));
    global.fetch = request as typeof fetch;

    await modelsApi.getModels();

    expect(request).toHaveBeenCalledWith('/api/models', expect.anything());
  });

  it("asks for the models a workspace's chats can run", async () => {
    const saved = { name: 'gpt-4o-mini', size: 0, modified_at: '2026-01-01T00:00:00Z' };
    const request = mock(async () => Response.json([saved]));
    global.fetch = request as typeof fetch;

    expect(await modelsApi.getModels('ws-1')).toEqual({ models: [saved] });
    expect(request).toHaveBeenCalledWith('/api/models?workspace_id=ws-1', expect.anything());
  });

  it('keeps the provider errors a partial inventory carries', async () => {
    const model = { name: 'flux1-dev.safetensors', size: 1, modified_at: '2024-01-01T00:00:00Z' };
    global.fetch = mock(async () =>
      Response.json({ models: [model], errors: { ollama: 'Failed to connect to Ollama' } })
    ) as typeof fetch;

    expect(await modelsApi.getModels()).toEqual({
      models: [model],
      errors: { ollama: 'Failed to connect to Ollama' },
    });
  });

  afterEach(() => {
    global.fetch = original;
  });

  it('encodes the complete model name when requesting details', async () => {
    const request = mock(async () => Response.json({ content: 'model', gguf_size: 123 }));
    global.fetch = request as typeof fetch;

    expect(await modelsApi.getModelInfo(name)).toEqual({ content: 'model', gguf_size: 123 });
    expect(request).toHaveBeenCalledWith(
      '/api/models/hf.co%2Fowner%2Frepository%3AQ4_K_M',
      expect.anything()
    );
  });

  it('requests disk usage from the dedicated endpoint', async () => {
    const disk = {
      used_bytes: 50,
      total_bytes: 100,
      available_bytes: 50,
      percent: 50,
    };
    const request = mock(async () => Response.json(disk));
    global.fetch = request as typeof fetch;

    expect(await modelsApi.getDisk()).toEqual(disk);
    expect(request).toHaveBeenCalledWith('/api/models/disk', expect.anything());
  });

  it('validates the structured training response', async () => {
    global.fetch = mock(async () =>
      Response.json({
        filename: null,
        quality: null,
        dataset: [],
        screening: {
          kept: 1,
          dropped: [{ filename: 'broken.png', reason: 'invalid' }],
        },
      })
    ) as typeof fetch;

    await expect(
      modelsApi.train({
        name: 'portrait',
        base: 'flux-schnell',
        images: [],
      })
    ).rejects.toThrow('Validation failed: screening.dropped.0.reason');
  });

  it('treats an empty training status as no job', async () => {
    const request = mock(async () => new Response(null, { status: 204 }));
    global.fetch = request as typeof fetch;

    expect(await modelsApi.trainJob()).toBeNull();
    expect(request).toHaveBeenCalledWith('/api/models/train', expect.anything());
  });

  it('polls until a 202 training job finishes', async () => {
    const running = {
      id: 'job-1',
      name: 'portrait',
      status: 'running',
    };
    const done = {
      id: 'job-1',
      name: 'portrait',
      status: 'succeeded',
      filename: 'portrait.safetensors',
      quality: null,
      dataset: [],
      screening: null,
    };
    const request = mock(async (_input: RequestInfo, init?: RequestInit) => {
      if (init?.method === 'POST') {
        return new Response(JSON.stringify(running), { status: 202 });
      }
      return Response.json(done);
    });
    global.fetch = request as typeof fetch;

    expect(
      await modelsApi.train({
        name: 'portrait',
        base: 'flux-schnell',
        images: [],
      })
    ).toEqual({
      filename: 'portrait.safetensors',
      quality: null,
      dataset: [],
      screening: null,
    });
    expect(request.mock.calls.some((call) => call[1]?.method === 'POST')).toBe(true);
    expect(request.mock.calls.some((call) => call[1]?.method !== 'POST')).toBe(true);
  });

  it('encodes the complete model name when deleting it', async () => {
    const request = mock(async () => new Response(null, { status: 204 }));
    global.fetch = request as typeof fetch;

    await modelsApi.deleteModel(name);
    expect(request).toHaveBeenCalledWith(
      '/api/models/hf.co%2Fowner%2Frepository%3AQ4_K_M',
      expect.objectContaining({ method: 'DELETE' })
    );
  });

  it("names the reason the server refuses a workspace's endpoint", async () => {
    const reason =
      "This workspace's AI endpoint can't be used: its host isn't one this instance allows endpoints on. Check AI Settings.";
    global.fetch = mock(async () =>
      Response.json({ success: false, error: reason }, { status: 409 })
    ) as unknown as typeof fetch;

    const failure = await modelsApi.getModels('ws-1').catch((error: unknown) => error);

    expect(failure).toBeInstanceOf(ApiError);
    expect((failure as ApiError).status).toBe(409);
    expect((failure as ApiError).message).toBe(`Failed to fetch models: ${reason}`);
  });

  it('falls back to the status when a refused listing carries no reason', async () => {
    global.fetch = mock(async () => new Response('', { status: 500 })) as unknown as typeof fetch;

    await expect(modelsApi.getModels()).rejects.toThrow('Failed to fetch models: 500');
  });
});
