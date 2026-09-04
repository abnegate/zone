import {
  GatherContextResponseSchema,
  KnowledgeEntrySchema,
  KnowledgeResponseSchema,
  SearchResponseSchema,
} from '../features/knowledge/schemas';
import type {
  CreateKnowledgeRequest,
  GatherContextRequest,
  KnowledgeEntry,
  KnowledgeResponse,
  SearchOptions,
  SearchResponse,
} from '../features/knowledge/types';
import { parse } from '../validation';

const API_BASE = import.meta.env.VITE_API_URL || '';

class KnowledgeApi {
  private getAccessToken: (() => string | null) | null = null;

  setGetAccessToken(fn: () => string | null) {
    this.getAccessToken = fn;
  }

  private getHeaders(): HeadersInit {
    const headers: HeadersInit = {
      'Content-Type': 'application/json',
    };
    if (this.getAccessToken) {
      const token = this.getAccessToken();
      if (token) {
        headers.Authorization = `Bearer ${token}`;
      }
    }
    return headers;
  }

  private async parseErrorResponse(response: Response): Promise<{ message?: string }> {
    try {
      const data = await response.json();
      return { message: data.message || data.error || data.detail };
    } catch {
      return {};
    }
  }

  // =============================================================================
  // Knowledge Base API
  // =============================================================================

  async getKnowledge(workspaceId: string): Promise<KnowledgeResponse> {
    const params = `?workspace_id=${encodeURIComponent(workspaceId)}`;
    const response = await fetch(`${API_BASE}/api/knowledge${params}`, {
      headers: this.getHeaders(),
    });
    if (!response.ok) {
      const errorData = await this.parseErrorResponse(response);
      throw new Error(errorData.message || `Failed to fetch knowledge: ${response.status}`);
    }
    return parse(KnowledgeResponseSchema, await response.json());
  }

  async createKnowledge(request: CreateKnowledgeRequest): Promise<KnowledgeEntry> {
    const response = await fetch(`${API_BASE}/api/knowledge`, {
      method: 'POST',
      headers: this.getHeaders(),
      body: JSON.stringify(request),
    });
    if (!response.ok) {
      const errorData = await this.parseErrorResponse(response);
      throw new Error(errorData.message || `Failed to create knowledge: ${response.status}`);
    }
    return parse(KnowledgeEntrySchema, await response.json());
  }

  async deleteKnowledge(id: string): Promise<void> {
    const response = await fetch(`${API_BASE}/api/knowledge/${encodeURIComponent(id)}`, {
      method: 'DELETE',
      headers: this.getHeaders(),
    });
    if (!response.ok) {
      const errorData = await this.parseErrorResponse(response);
      throw new Error(errorData.message || `Failed to delete knowledge: ${response.status}`);
    }
  }

  async refreshKnowledge(id: string): Promise<KnowledgeEntry> {
    const response = await fetch(`${API_BASE}/api/knowledge/${encodeURIComponent(id)}/refresh`, {
      method: 'POST',
      headers: this.getHeaders(),
    });
    if (!response.ok) {
      const errorData = await this.parseErrorResponse(response);
      throw new Error(errorData.message || `Failed to refresh knowledge: ${response.status}`);
    }
    return parse(KnowledgeEntrySchema, await response.json());
  }

  // =============================================================================
  // Context Search API
  // =============================================================================

  async searchContext(options: SearchOptions): Promise<SearchResponse> {
    const params = new URLSearchParams();
    params.set('workspace_id', options.workspace_id);
    params.set('q', options.query);
    if (options.mode) params.set('mode', options.mode);
    if (options.source_ids && options.source_ids.length > 0) {
      options.source_ids.forEach((id) => {
        params.append('source_ids', id);
      });
    }
    if (options.limit !== undefined) params.set('limit', options.limit.toString());

    const response = await fetch(`${API_BASE}/api/context/search?${params}`, {
      headers: this.getHeaders(),
    });
    if (!response.ok) {
      const errorData = await this.parseErrorResponse(response);
      throw new Error(errorData.message || `Failed to search context: ${response.status}`);
    }
    return parse(SearchResponseSchema, await response.json());
  }

  async gatherContext(request: GatherContextRequest): Promise<{ gathering_id: string }> {
    const response = await fetch(`${API_BASE}/api/context/gather`, {
      method: 'POST',
      headers: this.getHeaders(),
      body: JSON.stringify(request),
    });
    if (!response.ok) {
      const errorData = await this.parseErrorResponse(response);
      throw new Error(errorData.message || `Failed to gather context: ${response.status}`);
    }
    return parse(GatherContextResponseSchema, await response.json());
  }

  createContextGatheringWebSocket(gatheringId: string): WebSocket {
    let wsUrl: string;
    if (API_BASE) {
      const wsBase = API_BASE.replace(/^http/, 'ws');
      wsUrl = `${wsBase}/ws/context/${encodeURIComponent(gatheringId)}`;
    } else {
      const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
      wsUrl = `${protocol}//${window.location.host}/ws/context/${encodeURIComponent(gatheringId)}`;
    }
    return new WebSocket(wsUrl);
  }
}

export const knowledgeApi = new KnowledgeApi();
