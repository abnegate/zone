import { ApiError } from '../../../api/ApiError';
import { API_BASE, client } from '../../../api/client';
import { parse } from '../../../validation';
import { ScimRefusal } from './ScimRefusal';
import {
  CreateTokenRequestSchema,
  GroupResponseSchema,
  IssuedResponseSchema,
  MappingRequestSchema,
  ScimRefusalSchema,
  SettingsResponseSchema,
} from './schemas';
import type { GroupResponse, IssuedResponse, MappingRequest, ScimSettings } from './types';

function scimUrl(organizationId: string): string {
  return `${API_BASE}/api/organizations/${encodeURIComponent(organizationId)}/scim`;
}

function tokenUrl(organizationId: string, tokenId: string): string {
  return `${scimUrl(organizationId)}/tokens/${encodeURIComponent(tokenId)}`;
}

function mappingUrl(organizationId: string, groupId: string): string {
  return `${scimUrl(organizationId)}/groups/${encodeURIComponent(groupId)}/mapping`;
}

async function failure(response: Response, action: string): Promise<ApiError> {
  const body = await ApiError.body(response);
  const refusal = ScimRefusalSchema.safeParse(body);
  if (refusal.success) {
    return new ScimRefusal(refusal.data, response.status);
  }
  const { reason } = ApiError.refusal(body);
  return new ApiError(`${action}: ${reason ?? response.status}`, response.status);
}

async function send(url: string, init: RequestInit, action: string): Promise<Response> {
  const response = await fetch(url, { ...init, headers: client.getHeaders() });
  if (!response.ok) {
    throw await failure(response, action);
  }
  return response;
}

async function issued(response: Response): Promise<IssuedResponse> {
  return parse(IssuedResponseSchema, await response.json());
}

async function group(response: Response): Promise<GroupResponse> {
  return parse(GroupResponseSchema, await response.json());
}

export const scimApi = {
  async settings(organizationId: string): Promise<ScimSettings> {
    const response = await send(
      scimUrl(organizationId),
      { method: 'GET' },
      'Failed to load SCIM provisioning'
    );
    return parse(SettingsResponseSchema, await response.json());
  },

  async createToken(organizationId: string, name: string): Promise<IssuedResponse> {
    const response = await send(
      `${scimUrl(organizationId)}/tokens`,
      { method: 'POST', body: JSON.stringify(parse(CreateTokenRequestSchema, { name })) },
      'Failed to create the token'
    );
    return issued(response);
  },

  async rotateToken(organizationId: string, tokenId: string): Promise<IssuedResponse> {
    const response = await send(
      `${tokenUrl(organizationId, tokenId)}/rotations`,
      { method: 'POST' },
      'Failed to rotate the token'
    );
    return issued(response);
  },

  async revokeToken(organizationId: string, tokenId: string): Promise<void> {
    await send(
      tokenUrl(organizationId, tokenId),
      { method: 'DELETE' },
      'Failed to revoke the token'
    );
  },

  async mapGroup(
    organizationId: string,
    groupId: string,
    request: MappingRequest
  ): Promise<GroupResponse> {
    const response = await send(
      mappingUrl(organizationId, groupId),
      { method: 'PUT', body: JSON.stringify(parse(MappingRequestSchema, request)) },
      'Failed to map the group'
    );
    return group(response);
  },

  async unmapGroup(organizationId: string, groupId: string): Promise<GroupResponse> {
    const response = await send(
      mappingUrl(organizationId, groupId),
      { method: 'DELETE' },
      'Failed to unmap the group'
    );
    return group(response);
  },
};
