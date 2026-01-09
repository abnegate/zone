/**
 * Sources API
 * API client methods for source management.
 */

import { parse } from '../validation';
import {
  SourceResponseSchema,
  SourcesResponseSchema,
  SourceTypesResponseSchema,
  SourceVerifyResponseSchema,
} from '../features/sources/schemas';
import type {
  Source,
  SourceType,
  CreateSourceRequest,
  UpdateSourceRequest,
} from '../features/sources/types';
import type { SourceVerifyResponse, SourceTypesResponse } from '../features/sources/schemas';

export const API_BASE = process.env.REACT_APP_API_URL || '';

class SourcesApi {
  private getAccessToken: (() => string | null) | null = null;

  setGetAccessToken(fn: () => string | null) {
    this.getAccessToken = fn;
  }

  private getHeaders(): HeadersInit {
    const headers: HeadersInit = {
      'Content-Type': 'application/json',
    };
    const token = this.getAccessToken?.();
    if (token) {
      headers.Authorization = `Bearer ${token}`;
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

  async getSourceTypes(): Promise<SourceTypesResponse['types']> {
    const response = await fetch(`${API_BASE}/api/sources/types`, {
      headers: this.getHeaders(),
    });
    if (!response.ok) {
      const errorData = await this.parseErrorResponse(response);
      throw new Error(errorData.message || `Failed to fetch source types: ${response.status}`);
    }
    const data = parse(SourceTypesResponseSchema, await response.json());
    return data.types;
  }

  async getSources(type?: SourceType, activeOnly = false): Promise<Source[]> {
    const params = new URLSearchParams();
    if (type) params.set('type', type);
    if (activeOnly) params.set('active', 'true');
    const query = params.toString() ? `?${params}` : '';

    const response = await fetch(`${API_BASE}/api/sources${query}`, {
      headers: this.getHeaders(),
    });
    if (!response.ok) {
      const errorData = await this.parseErrorResponse(response);
      throw new Error(errorData.message || `Failed to fetch sources: ${response.status}`);
    }
    const data = parse(SourcesResponseSchema, await response.json());
    return data.sources;
  }

  async getSource(id: string): Promise<Source> {
    const response = await fetch(`${API_BASE}/api/sources/${encodeURIComponent(id)}`, {
      headers: this.getHeaders(),
    });
    if (!response.ok) {
      const errorData = await this.parseErrorResponse(response);
      throw new Error(errorData.message || `Failed to fetch source: ${response.status}`);
    }
    const data = parse(SourceResponseSchema, await response.json());
    return data.source;
  }

  async createSource(request: CreateSourceRequest): Promise<Source> {
    const response = await fetch(`${API_BASE}/api/sources`, {
      method: 'POST',
      headers: this.getHeaders(),
      body: JSON.stringify(request),
    });
    if (!response.ok) {
      const errorData = await this.parseErrorResponse(response);
      throw new Error(errorData.message || `Failed to create source: ${response.status}`);
    }
    const data = parse(SourceResponseSchema, await response.json());
    return data.source;
  }

  async updateSource(id: string, request: UpdateSourceRequest): Promise<Source> {
    const response = await fetch(`${API_BASE}/api/sources/${encodeURIComponent(id)}`, {
      method: 'PATCH',
      headers: this.getHeaders(),
      body: JSON.stringify(request),
    });
    if (!response.ok) {
      const errorData = await this.parseErrorResponse(response);
      throw new Error(errorData.message || `Failed to update source: ${response.status}`);
    }
    const data = parse(SourceResponseSchema, await response.json());
    return data.source;
  }

  async deleteSource(id: string): Promise<void> {
    const response = await fetch(`${API_BASE}/api/sources/${encodeURIComponent(id)}`, {
      method: 'DELETE',
      headers: this.getHeaders(),
    });
    if (!response.ok) {
      const errorData = await this.parseErrorResponse(response);
      throw new Error(errorData.message || `Failed to delete source: ${response.status}`);
    }
  }

  async verifySource(id: string): Promise<SourceVerifyResponse> {
    const response = await fetch(`${API_BASE}/api/sources/${encodeURIComponent(id)}/verify`, {
      method: 'POST',
      headers: this.getHeaders(),
    });
    if (!response.ok) {
      const errorData = await this.parseErrorResponse(response);
      throw new Error(errorData.message || `Failed to verify source: ${response.status}`);
    }
    return parse(SourceVerifyResponseSchema, await response.json());
  }
}

export const sourcesApi = new SourcesApi();
