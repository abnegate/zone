import type { z } from 'zod';
import type {
  DatasetConcernSchema,
  DatasetFindingSchema,
  DroppedImageSchema,
  DropReasonSchema,
  TrainFrameSchema,
  TrainQualitySchema,
  TrainRemediationSchema,
  TrainResultSchema,
  TrainScreeningSchema,
} from '../features/models/schemas';
import {
  BrowseResponseSchema,
  DiskUsageSchema,
  ModelsResponseSchema,
  TrainClipSchema,
  TrainJobSchema,
} from '../features/models/schemas';
import { captionBatches } from '../features/models/trainMedia';
import type {
  BrowseOptions,
  BrowseResponse,
  DiskUsage,
  ModelSizeOption,
  ModelSource,
  ModelsResponse,
} from '../features/models/types';
import { parse } from '../validation';
import { ApiError } from './ApiError';
import { client } from './client';

const API_BASE = import.meta.env.VITE_API_URL || '';

function bearerHeaders(): HeadersInit {
  const token = client.getAccessToken();
  return token ? { Authorization: `Bearer ${token}` } : {};
}

export type TrainFrame = z.infer<typeof TrainFrameSchema>;
export type TrainClip = z.infer<typeof TrainClipSchema>;

export type TrainQuality = z.infer<typeof TrainQualitySchema>;
export type DatasetConcern = z.infer<typeof DatasetConcernSchema>;
export type DatasetFinding = z.infer<typeof DatasetFindingSchema>;
export type DropReason = z.infer<typeof DropReasonSchema>;
export type DroppedImage = z.infer<typeof DroppedImageSchema>;
export type TrainRemediation = z.infer<typeof TrainRemediationSchema>;
export type TrainScreening = z.infer<typeof TrainScreeningSchema>;
export type TrainResult = z.infer<typeof TrainResultSchema>;
export type TrainJob = z.infer<typeof TrainJobSchema>;

function asTrainResult(job: TrainJob): TrainResult {
  return {
    filename: job.filename ?? null,
    quality: job.quality ?? null,
    dataset: job.dataset,
    screening: job.screening ?? null,
  };
}

function sleep(ms: number, signal?: AbortSignal): Promise<void> {
  return new Promise((resolve, reject) => {
    const timer = setTimeout(resolve, ms);
    const abort = () => {
      clearTimeout(timer);
      reject(new DOMException('The operation was aborted.', 'AbortError'));
    };
    if (signal?.aborted) {
      abort();
      return;
    }
    signal?.addEventListener('abort', abort, { once: true });
  });
}

type TrainImageUpload = {
  filename: string;
  caption: string;
  blob: Blob;
  before?: Blob;
  group?: number;
};

type TrainBody = {
  name: string;
  base: string;
  trigger?: string;
  subject?: 'person' | 'other' | 'language';
  method?: 'lora' | 'finetune' | 'pivotal' | 'video';
  provider?: 'local' | 'runpod';
  workspace_id?: string;
  images: TrainImageUpload[];
};

function trainForm(body: TrainBody, uploadId?: string): FormData {
  const form = new FormData();
  form.append('name', body.name);
  form.append('base', body.base);
  form.append('subject', body.subject ?? 'other');
  form.append('method', body.method ?? 'lora');
  form.append('provider', body.provider ?? 'local');
  if (body.trigger) form.append('trigger', body.trigger);
  if (uploadId) {
    form.append('upload_id', uploadId);
    return form;
  }
  form.append(
    'images',
    JSON.stringify(
      body.images.map(({ filename, caption, group }) => ({ filename, caption, group }))
    )
  );
  body.images.forEach((image, index) => {
    form.append(`image_${index}`, image.blob, image.filename);
    if (image.before) form.append(`before_${index}`, image.before, `before-${image.filename}`);
  });
  return form;
}

function appendForm(images: TrainImageUpload[]): FormData {
  const form = new FormData();
  form.append(
    'images',
    JSON.stringify(images.map(({ filename, caption, group }) => ({ filename, caption, group })))
  );
  images.forEach((image, index) => {
    form.append(`image_${index}`, image.blob, image.filename);
    if (image.before) form.append(`before_${index}`, image.before, `before-${image.filename}`);
  });
  return form;
}

async function trainFailure(response: Response, fallback: string): Promise<Error> {
  const payload = await response.json().catch(() => ({ error: fallback }));
  return new Error(payload.error || `${fallback}: ${response.status}`);
}

async function createTrainUpload(signal?: AbortSignal): Promise<string> {
  const response = await fetch(`${API_BASE}/api/models/train/uploads`, {
    method: 'POST',
    headers: bearerHeaders(),
    signal,
  });
  if (!response.ok) {
    throw await trainFailure(response, 'Training upload failed');
  }
  const payload = (await response.json()) as { id?: string };
  if (!payload.id) {
    throw new Error('Training upload failed');
  }
  return payload.id;
}

async function appendTrainUpload(
  uploadId: string,
  images: TrainImageUpload[],
  signal?: AbortSignal
): Promise<void> {
  const response = await fetch(
    `${API_BASE}/api/models/train/uploads/${encodeURIComponent(uploadId)}`,
    {
      method: 'POST',
      headers: bearerHeaders(),
      body: appendForm(images),
      signal,
    }
  );
  if (!response.ok) {
    throw await trainFailure(response, 'Training upload failed');
  }
}

async function dropTrainUpload(uploadId: string): Promise<void> {
  try {
    await fetch(`${API_BASE}/api/models/train/uploads/${encodeURIComponent(uploadId)}`, {
      method: 'DELETE',
      headers: bearerHeaders(),
    });
  } catch {
    return;
  }
}

async function postTrain(
  body: TrainBody,
  uploadId: string | undefined,
  signal: AbortSignal | undefined,
  onProgress: ((job: TrainJob) => void) | undefined
): Promise<TrainResult> {
  const query = body.workspace_id ? `?workspace_id=${encodeURIComponent(body.workspace_id)}` : '';
  const response = await fetch(`${API_BASE}/api/models/train${query}`, {
    method: 'POST',
    headers: bearerHeaders(),
    body: trainForm(body, uploadId),
    signal,
  });
  if (!response.ok) {
    throw await trainFailure(response, 'Training failed');
  }
  const job = parse(TrainJobSchema, await response.json());
  if (job.status === 'failed') {
    throw new Error(job.error || 'Training failed');
  }
  if (job.status === 'running' || response.status === 202) {
    onProgress?.(job);
    return modelsApi.waitTrain(signal, onProgress);
  }
  return asTrainResult(job);
}

/**
 * Models API
 * Provides methods for managing AI models: listing installed models,
 * browsing available models, pulling/deleting models, and monitoring pull progress.
 */
export const modelsApi = {
  /**
   * Get list of installed models, or the models a chat in `workspaceId` can run
   */
  async getModels(workspaceId?: string): Promise<ModelsResponse> {
    const query = workspaceId ? `?${new URLSearchParams({ workspace_id: workspaceId })}` : '';
    const response = await fetch(`${API_BASE}/api/models${query}`, {
      headers: client.getHeaders(),
    });
    if (!response.ok) {
      throw await ApiError.from(response, 'Failed to fetch models');
    }
    const text = await response.text();
    if (!text) {
      return { models: [] };
    }
    try {
      const data = JSON.parse(text);
      const wrapped = Array.isArray(data) ? { models: data } : data;
      return parse(ModelsResponseSchema, wrapped);
    } catch (e) {
      if (e instanceof SyntaxError) {
        throw new Error('Invalid response from server');
      }
      throw e;
    }
  },

  /**
   * Delete an installed model
   */
  async deleteModel(name: string): Promise<void> {
    const response = await fetch(`${API_BASE}/api/models/${encodeURIComponent(name)}`, {
      method: 'DELETE',
      headers: client.getHeaders(),
    });
    if (!response.ok) {
      throw new Error(`Failed to delete model: ${response.status}`);
    }
  },

  /**
   * Browse available models from a source
   * @param cursor - Pagination cursor for fetching next page (from previous response's next_cursor)
   */
  async browseModels(
    source: ModelSource,
    query = '',
    cursor?: string | null,
    limit = 20,
    options: BrowseOptions = {}
  ): Promise<BrowseResponse> {
    const params = new URLSearchParams({
      source,
      q: query,
      limit: limit.toString(),
      sort: options.sort ?? 'relevance',
    });
    if (cursor) {
      params.set('cursor', cursor);
    }
    if (options.family) {
      params.set('family', options.family);
    }
    if (options.size && options.size !== 'all') {
      params.set('size', options.size);
    }
    if (options.medium && options.medium !== 'all') {
      params.set('medium', options.medium);
    }
    const response = await fetch(`${API_BASE}/api/models?${params}`, {
      headers: client.getHeaders(),
    });
    if (!response.ok) {
      throw new Error(`Failed to browse models: ${response.status}`);
    }
    const data = await response.json();
    return parse(BrowseResponseSchema, data);
  },

  /**
   * Get detailed information about a model
   */
  async getModelInfo(modelId: string): Promise<{
    content: string | null;
    gguf_size: number | null;
    sizes?: ModelSizeOption[] | null;
  }> {
    const response = await fetch(`${API_BASE}/api/models/${encodeURIComponent(modelId)}`, {
      headers: client.getHeaders(),
    });
    if (!response.ok) {
      throw new Error(`Failed to fetch model info: ${response.status}`);
    }
    const data = await response.json();
    return { content: data.content, gguf_size: data.gguf_size, sizes: data.sizes };
  },

  /**
   * Host filesystem usage for the volume that stores models
   */
  async getDisk(): Promise<DiskUsage> {
    const response = await fetch(`${API_BASE}/api/models/disk`, {
      headers: client.getHeaders(),
    });
    if (!response.ok) {
      throw new Error(`Failed to fetch disk usage: ${response.status}`);
    }
    return parse(DiskUsageSchema, await response.json());
  },

  async captions(body: {
    trigger?: string;
    images: Array<{ filename: string; caption: string; blob: Blob; group?: number }>;
  }): Promise<{ captions: string[] }> {
    const form = new FormData();
    if (body.trigger) form.append('trigger', body.trigger);
    form.append(
      'images',
      JSON.stringify(
        body.images.map(({ filename, caption, group }) => ({ filename, caption, group }))
      )
    );
    body.images.forEach((image, index) => {
      form.append(`image_${index}`, image.blob, image.filename);
    });
    const response = await fetch(`${API_BASE}/api/models/train/captions`, {
      method: 'POST',
      headers: bearerHeaders(),
      body: form,
    });
    if (!response.ok) {
      const payload = await response.json().catch(() => ({ error: 'Captioning failed' }));
      throw new Error(payload.error || `Failed to caption: ${response.status}`);
    }
    return response.json();
  },

  async trainJob(signal?: AbortSignal): Promise<TrainJob | null> {
    const response = await fetch(`${API_BASE}/api/models/train`, {
      headers: client.getHeaders(),
      signal,
    });
    if (response.status === 204) return null;
    if (!response.ok) {
      const payload = await response
        .json()
        .catch(() => ({ error: 'Could not read training status' }));
      throw new Error(payload.error || `Failed to read training status: ${response.status}`);
    }
    return parse(TrainJobSchema, await response.json());
  },

  async dismissTrain(): Promise<void> {
    const response = await fetch(`${API_BASE}/api/models/train`, {
      method: 'DELETE',
      headers: client.getHeaders(),
    });
    if (response.status === 204) return;
    const payload = await response
      .json()
      .catch(() => ({ error: 'Could not dismiss training status' }));
    throw new Error(payload.error || `Failed to dismiss training status: ${response.status}`);
  },

  async waitTrain(
    signal?: AbortSignal,
    onProgress?: (job: TrainJob) => void
  ): Promise<TrainResult> {
    for (;;) {
      const job = await modelsApi.trainJob(signal);
      if (!job) {
        throw new Error('Training job disappeared');
      }
      onProgress?.(job);
      if (job.status === 'failed') {
        throw new Error(job.error || 'Training failed');
      }
      if (job.status !== 'running') {
        return asTrainResult(job);
      }
      await sleep(2000, signal);
    }
  },

  async train(
    body: TrainBody,
    signal?: AbortSignal,
    onProgress?: (job: TrainJob) => void
  ): Promise<TrainResult> {
    if (body.images.length === 0) {
      return postTrain(body, undefined, signal, onProgress);
    }
    const uploadId = await createTrainUpload(signal);
    try {
      for (const batch of captionBatches(body.images)) {
        await appendTrainUpload(uploadId, batch, signal);
      }
      return await postTrain(body, uploadId, signal, onProgress);
    } catch (error) {
      await dropTrainUpload(uploadId);
      throw error;
    }
  },

  /**
   * Pull training frames out of a video. The server samples above the kept rate,
   * keeps the sharpest frame of each moment, drops repeats of a shot it already
   * has, and crops what is left around whatever moved.
   */
  async frames(body: {
    filename: string;
    blob: Blob;
    fps?: number;
    mirror?: boolean;
  }): Promise<TrainClip> {
    const form = new FormData();
    form.append('filename', body.filename);
    form.append('video', body.blob, body.filename);
    if (body.fps != null) form.append('fps', String(body.fps));
    if (body.mirror != null) form.append('mirror', String(body.mirror));
    const response = await fetch(`${API_BASE}/api/models/train/frames`, {
      method: 'POST',
      headers: bearerHeaders(),
      body: form,
    });
    if (!response.ok) {
      const payload = await response.json().catch(() => ({ error: 'Frame extraction failed' }));
      throw new Error(payload.error || `Failed to read the video: ${response.status}`);
    }
    return parse(TrainClipSchema, await response.json());
  },

  async trainBases(): Promise<
    Array<{
      id: string;
      label: string;
      edit: boolean;
      subject?: 'person' | 'other' | 'language';
      finetune?: boolean;
    }>
  > {
    const response = await fetch(`${API_BASE}/api/models/train/bases`, {
      headers: client.getHeaders(),
    });
    if (!response.ok) {
      throw new Error(`Failed to list training bases: ${response.status}`);
    }
    return response.json();
  },

  /**
   * Create a WebSocket connection for pulling a model
   */
  createPullWebSocket(modelName: string): WebSocket {
    let wsUrl: string;
    if (API_BASE) {
      // Development: use configured API URL
      const wsBase = API_BASE.replace(/^http/, 'ws');
      wsUrl = `${wsBase}/ws/pull?model=${encodeURIComponent(modelName)}`;
    } else {
      // Production: use current host
      const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
      wsUrl = `${protocol}//${window.location.host}/ws/pull?model=${encodeURIComponent(modelName)}`;
    }
    return new WebSocket(wsUrl);
  },
};
