import { ApiError } from '../../api/ApiError';
import { API_BASE, client } from '../../api/client';
import { HttpStatus } from '../../api/HttpStatus';
import { parse } from '../../validation';
import { LicenseRefusal } from './LicenseRefusal';
import { LicenseRefusalSchema, LicenseResponseSchema, SetLicenseRequestSchema } from './schemas';
import type { LicenseResponse } from './types';

function licenseUrl(): string {
  return `${API_BASE}/api/license`;
}

async function failure(response: Response, action: string): Promise<ApiError> {
  if (response.status === HttpStatus.BadRequest) {
    const body: unknown = await response
      .clone()
      .json()
      .catch(() => undefined);
    const refusal = LicenseRefusalSchema.safeParse(body);
    if (refusal.success) {
      return new LicenseRefusal(refusal.data);
    }
  }
  return ApiError.from(response, action);
}

async function send(init: RequestInit, action: string): Promise<LicenseResponse> {
  const response = await fetch(licenseUrl(), { ...init, headers: client.getHeaders() });
  if (!response.ok) {
    throw await failure(response, action);
  }
  return parse(LicenseResponseSchema, await response.json());
}

export const licenseApi = {
  async get(): Promise<LicenseResponse> {
    return send({ method: 'GET' }, 'Failed to load the license');
  },

  async set(key: string): Promise<LicenseResponse> {
    return send(
      { method: 'PUT', body: JSON.stringify(parse(SetLicenseRequestSchema, { key })) },
      'Failed to save the license'
    );
  },

  async remove(): Promise<LicenseResponse> {
    return send({ method: 'DELETE' }, 'Failed to remove the license');
  },
};
