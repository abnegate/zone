import { ApiError } from '../../../api/ApiError';
import { API_BASE, client } from '../../../api/client';
import { parse } from '../../../validation';
import { PolicyRefusal } from './PolicyRefusal';
import {
  OrganizationPolicyResponseSchema,
  PolicyRefusalSchema,
  UpdatePolicyRequestSchema,
  WorkspacePolicyResponseSchema,
} from './schemas';
import type {
  OrganizationPolicyResponse,
  UpdatePolicyRequest,
  WorkspacePolicyResponse,
} from './types';

const POLICY_PATH = '/agent-policy';

function organizationUrl(organizationId: string): string {
  return `${API_BASE}/api/organizations/${encodeURIComponent(organizationId)}${POLICY_PATH}`;
}

function workspaceUrl(organizationId: string, workspaceId: string): string {
  return `${API_BASE}/api/organizations/${encodeURIComponent(organizationId)}/workspaces/${encodeURIComponent(workspaceId)}${POLICY_PATH}`;
}

async function failure(response: Response, action: string): Promise<ApiError> {
  const body: unknown = await response
    .clone()
    .json()
    .catch(() => undefined);
  const refusal = PolicyRefusalSchema.safeParse(body);
  if (refusal.success) {
    return new PolicyRefusal(refusal.data, response.status);
  }
  return ApiError.from(response, action);
}

async function send(url: string, init: RequestInit, action: string): Promise<unknown> {
  const response = await fetch(url, { ...init, headers: client.getHeaders() });
  if (!response.ok) {
    throw await failure(response, action);
  }
  return response.json();
}

function patch(request: UpdatePolicyRequest): RequestInit {
  return { method: 'PATCH', body: JSON.stringify(parse(UpdatePolicyRequestSchema, request)) };
}

export const policyApi = {
  async organization(organizationId: string): Promise<OrganizationPolicyResponse> {
    return parse(
      OrganizationPolicyResponseSchema,
      await send(
        organizationUrl(organizationId),
        { method: 'GET' },
        'Failed to load the agent policy'
      )
    );
  },

  async updateOrganization(
    organizationId: string,
    request: UpdatePolicyRequest
  ): Promise<OrganizationPolicyResponse> {
    return parse(
      OrganizationPolicyResponseSchema,
      await send(organizationUrl(organizationId), patch(request), 'Failed to save the agent policy')
    );
  },

  async workspace(organizationId: string, workspaceId: string): Promise<WorkspacePolicyResponse> {
    return parse(
      WorkspacePolicyResponseSchema,
      await send(
        workspaceUrl(organizationId, workspaceId),
        { method: 'GET' },
        'Failed to load the agent policy'
      )
    );
  },

  async updateWorkspace(
    organizationId: string,
    workspaceId: string,
    request: UpdatePolicyRequest
  ): Promise<WorkspacePolicyResponse> {
    return parse(
      WorkspacePolicyResponseSchema,
      await send(
        workspaceUrl(organizationId, workspaceId),
        patch(request),
        'Failed to save the agent policy'
      )
    );
  },
};
