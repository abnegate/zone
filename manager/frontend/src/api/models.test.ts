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

  it('dismisses a finished training job', async () => {
    const request = mock(async () => new Response(null, { status: 204 }));
    global.fetch = request as typeof fetch;

    await modelsApi.dismissTrain();
    expect(request).toHaveBeenCalledWith(
      '/api/models/train',
      expect.objectContaining({ method: 'DELETE' })
    );
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

  it('reports step progress while a 202 training job is still running', async () => {
    const running = {
      id: 'job-1',
      name: 'portrait',
      status: 'running',
      step: 40,
      total: 400,
      eta_seconds: 180,
      started_at: '2026-10-03T12:00:00Z',
    };
    const done = {
      ...running,
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
    const seen: Array<{ step?: number; eta_seconds?: number | null }> = [];

    expect(
      await modelsApi.train(
        {
          name: 'portrait',
          base: 'flux-schnell',
          images: [],
        },
        undefined,
        (job) => seen.push({ step: job.step, eta_seconds: job.eta_seconds })
      )
    ).toEqual({
      filename: 'portrait.safetensors',
      quality: null,
      dataset: [],
      screening: null,
    });
    expect(seen[0]).toEqual({ step: 40, eta_seconds: 180 });
  });

  it('posts training images in staged batches then trains with the upload id', async () => {
    const request = mock(async (input: RequestInfo, init?: RequestInit) => {
      const url = String(input);
      expect((init?.headers as Record<string, string>)?.['Content-Type']).toBeUndefined();
      if (url.endsWith('/api/models/train/uploads') && init?.method === 'POST') {
        return Response.json({ id: 'upload-1' });
      }
      if (url.endsWith('/api/models/train/uploads/upload-1') && init?.method === 'POST') {
        expect(init?.body).toBeInstanceOf(FormData);
        const form = init.body as FormData;
        expect(form.get('image_0')).toBeInstanceOf(Blob);
        expect(JSON.parse(String(form.get('images')))).toEqual([
          { filename: 'shot.png', caption: 'a person' },
        ]);
        return Response.json({ id: 'upload-1', received: 1 });
      }
      if (url === '/api/models/train' && init?.method === 'POST') {
        expect(init?.body).toBeInstanceOf(FormData);
        const form = init.body as FormData;
        expect(form.get('name')).toBe('portrait');
        expect(form.get('subject')).toBe('other');
        expect(form.get('method')).toBe('lora');
        expect(form.get('provider')).toBe('local');
        expect(form.get('upload_id')).toBe('upload-1');
        expect(form.get('image_0')).toBeNull();
        return Response.json({
          id: 'job-1',
          name: 'portrait',
          status: 'succeeded',
          filename: 'portrait.safetensors',
          quality: null,
          dataset: [],
          screening: null,
        });
      }
      if (url.includes('/train/frames')) {
        const form = init?.body;
        expect(form).toBeInstanceOf(FormData);
        expect((form as FormData).get('video')).toBeInstanceOf(Blob);
        return Response.json({ sampled: 1, sampled_fps: 8, frames: [] });
      }
      throw new Error(`unexpected ${init?.method} ${url}`);
    });
    global.fetch = request as typeof fetch;
    const blob = new Blob(['pixels'], { type: 'image/png' });

    await modelsApi.train({
      name: 'portrait',
      base: 'flux-schnell',
      images: [{ filename: 'shot.png', caption: 'a person', blob }],
    });
    await modelsApi.frames({ filename: 'walk.mp4', blob: new Blob(['clip']), mirror: true });
    expect(request.mock.calls.map((call) => [call[1]?.method ?? 'GET', String(call[0])])).toEqual([
      ['POST', '/api/models/train/uploads'],
      ['POST', '/api/models/train/uploads/upload-1'],
      ['POST', '/api/models/train'],
      ['POST', '/api/models/train/frames'],
    ]);
  });

  it('drops a staged upload when a later batch fails', async () => {
    const request = mock(async (input: RequestInfo, init?: RequestInit) => {
      const url = String(input);
      if (url.endsWith('/api/models/train/uploads') && init?.method === 'POST') {
        return Response.json({ id: 'upload-1' });
      }
      if (url.endsWith('/api/models/train/uploads/upload-1') && init?.method === 'POST') {
        return Response.json({ error: 'image is empty' }, { status: 400 });
      }
      if (url.endsWith('/api/models/train/uploads/upload-1') && init?.method === 'DELETE') {
        return new Response(null, { status: 204 });
      }
      throw new Error(`unexpected ${init?.method} ${url}`);
    });
    global.fetch = request as typeof fetch;

    await expect(
      modelsApi.train({
        name: 'portrait',
        base: 'flux-schnell',
        images: [{ filename: 'shot.png', caption: 'a person', blob: new Blob(['pixels']) }],
      })
    ).rejects.toThrow('image is empty');
    expect(request.mock.calls.map((call) => [call[1]?.method ?? 'GET', String(call[0])])).toEqual([
      ['POST', '/api/models/train/uploads'],
      ['POST', '/api/models/train/uploads/upload-1'],
      ['DELETE', '/api/models/train/uploads/upload-1'],
    ]);
  });

  it('posts a person fine-tune as subject and method fields', async () => {
    const request = mock(async (_input: RequestInfo, init?: RequestInit) => {
      expect(init?.body).toBeInstanceOf(FormData);
      const form = init?.body as FormData;
      expect(form.get('name')).toBe('portrait');
      expect(form.get('base')).toBe('sdxl-people');
      expect(form.get('subject')).toBe('person');
      expect(form.get('method')).toBe('finetune');
      expect(form.get('provider')).toBe('local');
      expect(form.get('trigger')).toBe('zne person');
      return Response.json({
        id: 'job-1',
        name: 'portrait',
        status: 'succeeded',
        filename: 'portrait.safetensors',
        quality: null,
        dataset: [],
        screening: null,
      });
    });
    global.fetch = request as typeof fetch;

    await modelsApi.train({
      name: 'portrait',
      base: 'sdxl-people',
      trigger: 'zne person',
      subject: 'person',
      method: 'finetune',
      images: [],
    });
    expect(request).toHaveBeenCalledTimes(1);
  });

  it('posts Runpod compute and the workspace on the train URL', async () => {
    const request = mock(async (input: RequestInfo, init?: RequestInit) => {
      expect(String(input)).toBe('/api/models/train?workspace_id=ws-1');
      const form = init?.body as FormData;
      expect(form.get('provider')).toBe('runpod');
      expect(form.get('subject')).toBe('person');
      return Response.json({
        id: 'job-1',
        name: 'portrait',
        status: 'succeeded',
        filename: 'portrait.safetensors',
        quality: null,
        dataset: [],
        screening: null,
        provider: 'runpod',
        gpu: 'A40',
      });
    });
    global.fetch = request as typeof fetch;

    await modelsApi.train({
      name: 'portrait',
      base: 'sdxl-people',
      trigger: 'zne person',
      subject: 'person',
      method: 'finetune',
      provider: 'runpod',
      workspace_id: 'ws-1',
      images: [],
    });
    expect(request).toHaveBeenCalledTimes(1);
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
