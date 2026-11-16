import { ApiError } from '../../../api/ApiError';
import { API_BASE, client } from '../../../api/client';
import { parse } from '../../../validation';
import { SsoRefusal } from './SsoRefusal';
import {
  CreateProviderRequestSchema,
  EnforcementRequestSchema,
  EnforcementResponseSchema,
  ProviderResponseSchema,
  SettingsResponseSchema,
  SsoRefusalSchema,
  UpdateProviderRequestSchema,
} from './schemas';
import type {
  CreateProviderRequest,
  EnforcementResponse,
  ProviderResponse,
  SettingsResponse,
  UpdateProviderRequest,
} from './types';

function ssoUrl(path = ''): string {
  return `${API_BASE}/api/sso${path}`;
}

function providerUrl(providerId: string): string {
  return ssoUrl(`/providers/${encodeURIComponent(providerId)}`);
}

async function failure(response: Response, action: string): Promise<ApiError> {
  const body: unknown = await response
    .clone()
    .json()
    .catch(() => undefined);
  const refusal = SsoRefusalSchema.safeParse(body);
  if (refusal.success) {
    return new SsoRefusal(refusal.data, response.status);
  }
  return ApiError.from(response, action);
}

async function send(url: string, init: RequestInit, action: string): Promise<Response> {
  const response = await fetch(url, { ...init, headers: client.getHeaders() });
  if (!response.ok) {
    throw await failure(response, action);
  }
  return response;
}

export const ssoApi = {
  async get(): Promise<SettingsResponse> {
    const response = await send(ssoUrl(), { method: 'GET' }, 'Failed to load single sign-on');
    return parse(SettingsResponseSchema, await response.json());
  },

  async createProvider(request: CreateProviderRequest): Promise<ProviderResponse> {
    const response = await send(
      ssoUrl('/providers'),
      { method: 'POST', body: JSON.stringify(parse(CreateProviderRequestSchema, request)) },
      'Failed to add the provider'
    );
    return parse(ProviderResponseSchema, await response.json());
  },

  async updateProvider(
    providerId: string,
    request: UpdateProviderRequest
  ): Promise<ProviderResponse> {
    const response = await send(
      providerUrl(providerId),
      { method: 'PATCH', body: JSON.stringify(parse(UpdateProviderRequestSchema, request)) },
      'Failed to save the provider'
    );
    return parse(ProviderResponseSchema, await response.json());
  },

  async deleteProvider(providerId: string): Promise<void> {
    await send(providerUrl(providerId), { method: 'DELETE' }, 'Failed to delete the provider');
  },

  async setEnforcement(required: boolean): Promise<EnforcementResponse> {
    const response = await send(
      ssoUrl('/enforcement'),
      { method: 'PUT', body: JSON.stringify(parse(EnforcementRequestSchema, { required })) },
      'Failed to change single sign-on enforcement'
    );
    return parse(EnforcementResponseSchema, await response.json());
  },
};
