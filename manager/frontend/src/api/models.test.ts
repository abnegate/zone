import { afterEach, describe, expect, it, mock } from 'bun:test';
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

  it('encodes the complete model name when deleting it', async () => {
    const request = mock(async () => new Response(null, { status: 204 }));
    global.fetch = request as typeof fetch;

    await modelsApi.deleteModel(name);
    expect(request).toHaveBeenCalledWith(
      '/api/models/hf.co%2Fowner%2Frepository%3AQ4_K_M',
      expect.objectContaining({ method: 'DELETE' })
    );
  });
});
