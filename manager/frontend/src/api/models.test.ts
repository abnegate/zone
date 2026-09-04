import { afterEach, describe, expect, it, mock } from 'bun:test';
import { modelsApi } from './models';

describe('Namespaced model requests', () => {
  const original = global.fetch;
  const name = 'hf.co/owner/repository:Q4_K_M';

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
