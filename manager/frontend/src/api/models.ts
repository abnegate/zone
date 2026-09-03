import { BrowseResponseSchema, ModelsResponseSchema } from '../features/models/schemas';
import type {
  BrowseOptions,
  BrowseResponse,
  ModelSource,
  ModelsResponse,
} from '../features/models/types';
import { parse } from '../validation';
import { client } from './client';

const API_BASE = import.meta.env.VITE_API_URL || '';

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
  async getModelInfo(
    modelId: string
  ): Promise<{ content: string | null; gguf_size: number | null }> {
    // modelId may contain slashes (e.g., "author/model"), don't encode them
    const response = await fetch(`${API_BASE}/api/models/${modelId}`, {
      headers: client.getHeaders(),
    });
    if (!response.ok) {
      throw new Error(`Failed to fetch model info: ${response.status}`);
    }
    const data = await response.json();
    return { content: data.content, gguf_size: data.gguf_size };
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
