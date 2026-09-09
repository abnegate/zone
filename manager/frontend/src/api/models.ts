import type { z } from 'zod';
import type { TrainFrameSchema } from '../features/models/schemas';
import {
  BrowseResponseSchema,
  DiskUsageSchema,
  ModelsResponseSchema,
  TrainClipSchema,
} from '../features/models/schemas';
import type {
  BrowseOptions,
  BrowseResponse,
  DiskUsage,
  ModelSizeOption,
  ModelSource,
  ModelsResponse,
} from '../features/models/types';
import { parse } from '../validation';
import { client } from './client';

const API_BASE = import.meta.env.VITE_API_URL || '';

export type TrainFrame = z.infer<typeof TrainFrameSchema>;
export type TrainClip = z.infer<typeof TrainClipSchema>;

export type TrainQuality = {
  improvement: number;
  checkpoint: string;
  measured: boolean;
  calibration: 'flux_health_bands' | 'uncalibrated';
};

export type DatasetConcern = 'too_few' | 'low_variety' | 'low_pose_variety' | 'mixed_subjects';

export type DatasetFinding = {
  concern: DatasetConcern;
  detail: string;
};

export type DropReason = 'duplicate' | 'blurred' | 'small';

export type DroppedImage = {
  filename: string;
  reason: DropReason;
};

export type TrainRemediation = {
  filename: string;
  reason: DropReason;
  outcome: 'used' | 'still_rejected' | 'failed';
};

export type TrainScreening = {
  kept: number;
  dropped: DroppedImage[];
  attempted?: TrainRemediation[];
};

export type TrainResult = {
  filename: string | null;
  quality: TrainQuality | null;
  dataset?: DatasetFinding[];
  screening?: TrainScreening | null;
};

/**
 * Models API
 * Provides methods for managing AI models: listing installed models,
 * browsing available models, pulling/deleting models, and monitoring pull progress.
 */
export const modelsApi = {
  /**
   * Get list of installed models
   */
  async getModels(): Promise<ModelsResponse> {
    const response = await fetch(`${API_BASE}/api/models`, {
      headers: client.getHeaders(),
    });
    if (!response.ok) {
      throw new Error(`Failed to fetch models: ${response.status}`);
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
    images: Array<{ filename: string; caption: string; bytes_base64: string; group?: number }>;
  }): Promise<{ captions: string[] }> {
    const response = await fetch(`${API_BASE}/api/models/train/captions`, {
      method: 'POST',
      headers: { ...client.getHeaders(), 'Content-Type': 'application/json' },
      body: JSON.stringify(body),
    });
    if (!response.ok) {
      const payload = await response.json().catch(() => ({ error: 'Captioning failed' }));
      throw new Error(payload.error || `Failed to caption: ${response.status}`);
    }
    return response.json();
  },

  async train(body: {
    name: string;
    base: string;
    trigger?: string;
    images: Array<{
      filename: string;
      caption: string;
      bytes_base64: string;
      before_base64?: string;
      group?: number;
    }>;
  }): Promise<TrainResult> {
    const response = await fetch(`${API_BASE}/api/models/train`, {
      method: 'POST',
      headers: { ...client.getHeaders(), 'Content-Type': 'application/json' },
      body: JSON.stringify(body),
    });
    if (!response.ok) {
      const payload = await response.json().catch(() => ({ error: 'Training failed' }));
      throw new Error(payload.error || `Failed to train: ${response.status}`);
    }
    return response.json();
  },

  /**
   * Pull training frames out of a video. The server samples above the kept rate,
   * keeps the sharpest frame of each moment, drops repeats of a shot it already
   * has, and crops what is left around whatever moved.
   */
  async frames(body: {
    filename: string;
    bytes_base64: string;
    fps?: number;
    mirror?: boolean;
  }): Promise<TrainClip> {
    const response = await fetch(`${API_BASE}/api/models/train/frames`, {
      method: 'POST',
      headers: { ...client.getHeaders(), 'Content-Type': 'application/json' },
      body: JSON.stringify(body),
    });
    if (!response.ok) {
      const payload = await response.json().catch(() => ({ error: 'Frame extraction failed' }));
      throw new Error(payload.error || `Failed to read the video: ${response.status}`);
    }
    return parse(TrainClipSchema, await response.json());
  },

  async trainBases(): Promise<Array<{ id: string; label: string; edit: boolean }>> {
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
