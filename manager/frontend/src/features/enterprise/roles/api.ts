import { ApiError } from '../../../api/ApiError';
import { API_BASE, client } from '../../../api/client';
import { parse } from '../../../validation';
import { RoleRefusal } from './RoleRefusal';
import {
  CreateRoleRequestSchema,
  RoleRefusalSchema,
  RoleResponseSchema,
  RolesResponseSchema,
  UpdateRoleRequestSchema,
} from './schemas';
import type { CreateRoleRequest, CustomRole, RolesResponse, UpdateRoleRequest } from './types';

function rolesUrl(organizationId: string): string {
  return `${API_BASE}/api/organizations/${encodeURIComponent(organizationId)}/roles`;
}

function roleUrl(organizationId: string, roleId: string): string {
  return `${rolesUrl(organizationId)}/${encodeURIComponent(roleId)}`;
}

async function failure(response: Response, action: string): Promise<ApiError> {
  const body = await ApiError.body(response);
  const refusal = RoleRefusalSchema.safeParse(body);
  if (refusal.success) {
    return new RoleRefusal(refusal.data, response.status);
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

async function role(response: Response): Promise<CustomRole> {
  return parse(RoleResponseSchema, await response.json()).role;
}

export const rolesApi = {
  async list(organizationId: string): Promise<RolesResponse> {
    const response = await send(
      rolesUrl(organizationId),
      { method: 'GET' },
      'Failed to load the roles'
    );
    return parse(RolesResponseSchema, await response.json());
  },

  async create(organizationId: string, request: CreateRoleRequest): Promise<CustomRole> {
    const response = await send(
      rolesUrl(organizationId),
      { method: 'POST', body: JSON.stringify(parse(CreateRoleRequestSchema, request)) },
      'Failed to create the role'
    );
    return role(response);
  },

  async update(
    organizationId: string,
    roleId: string,
    request: UpdateRoleRequest
  ): Promise<CustomRole> {
    const response = await send(
      roleUrl(organizationId, roleId),
      { method: 'PATCH', body: JSON.stringify(parse(UpdateRoleRequestSchema, request)) },
      'Failed to save the role'
    );
    return role(response);
  },

  async remove(organizationId: string, roleId: string): Promise<void> {
    await send(roleUrl(organizationId, roleId), { method: 'DELETE' }, 'Failed to delete the role');
  },
};
