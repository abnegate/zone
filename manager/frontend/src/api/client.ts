import type { BrowseResponse, ModelSource, ModelsResponse } from '../types';

// In development, set REACT_APP_API_URL=http://localhost:8000
// In production (served by backend), leave empty to use relative URLs
const API_BASE = process.env.REACT_APP_API_URL || '';

class Client {
  private apiKey: string | null = null;

  setApiKey(key: string | null) {
    this.apiKey = key;
  }

  private getHeaders(): HeadersInit {
    const headers: HeadersInit = {
      'Content-Type': 'application/json',
    };
    if (this.apiKey) {
      headers.Authorization = `Bearer ${this.apiKey}`;
    }
    return headers;
  }

  async getModels(): Promise<ModelsResponse> {
    const response = await fetch(`${API_BASE}/api/models`, {
      headers: this.getHeaders(),
    });
    if (!response.ok) {
      throw new Error(`Failed to fetch models: ${response.status}`);
    }
    const text = await response.text();
    if (!text) {
      return { models: [] };
    }
    try {
      return JSON.parse(text);
    } catch {
      throw new Error('Invalid response from server');
    }
  }

  async deleteModel(name: string): Promise<void> {
    const response = await fetch(`${API_BASE}/api/models/${encodeURIComponent(name)}`, {
      method: 'DELETE',
      headers: this.getHeaders(),
    });
    if (!response.ok) {
      throw new Error(`Failed to delete model: ${response.status}`);
    }
  }

  async browseModels(
    source: ModelSource,
    query = '',
    offset = 0,
    limit = 20
  ): Promise<BrowseResponse> {
    const params = new URLSearchParams({
      source,
      q: query,
      offset: offset.toString(),
      limit: limit.toString(),
    });
    const response = await fetch(`${API_BASE}/api/models?${params}`, {
      headers: this.getHeaders(),
    });
    if (!response.ok) {
      throw new Error(`Failed to browse models: ${response.status}`);
    }
    return response.json();
  }

  async getModelInfo(modelId: string): Promise<{ content: string | null; gguf_size: number | null }> {
    // modelId may contain slashes (e.g., "author/model"), don't encode them
    const response = await fetch(`${API_BASE}/api/models/${modelId}`, {
      headers: this.getHeaders(),
    });
    if (!response.ok) {
      throw new Error(`Failed to fetch model info: ${response.status}`);
    }
    const data = await response.json();
    return { content: data.content, gguf_size: data.gguf_size };
  }

  createPullWebSocket(modelName: string): WebSocket {
    let wsUrl: string;
    if (API_BASE) {
      // Development: use configured API URL
      const wsBase = API_BASE.replace(/^http/, 'ws');
      wsUrl = `${wsBase}/ws/pull?model=${encodeURIComponent(modelName)}&api_key=${encodeURIComponent(this.apiKey || '')}`;
    } else {
      // Production: use current host
      const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
      wsUrl = `${protocol}//${window.location.host}/ws/pull?model=${encodeURIComponent(modelName)}&api_key=${encodeURIComponent(this.apiKey || '')}`;
    }
    return new WebSocket(wsUrl);
  }
}

export const client = new Client();
