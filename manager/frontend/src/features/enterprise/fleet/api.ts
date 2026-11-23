import { ApiError } from '../../../api/ApiError';
import { API_BASE, client } from '../../../api/client';
import { parse } from '../../../validation';
import { FleetRefusal } from './FleetRefusal';
import {
  fromOrganization,
  fromWorkspace,
  type Placement,
  PlacementKind,
  type PlacementScope,
} from './placement';
import {
  FleetPolicyResponseSchema,
  FleetRefusalSchema,
  OrganizationRunnerLabelsResponseSchema,
  RunnerResponseSchema,
  RunnersResponseSchema,
  UpdateFleetPolicyRequestSchema,
  UpdateRunnerLabelsRequestSchema,
  UpdateRunnerRequestSchema,
  WorkspaceRunnerLabelsResponseSchema,
} from './schemas';
import type {
  FleetPolicyResponse,
  RunnerResponse,
  RunnersResponse,
  UpdateFleetPolicyRequest,
  UpdateRunnerLabelsRequest,
  UpdateRunnerRequest,
} from './types';

const POLICY_PATH = '/api/fleet-policy';
const RUNNERS_PATH = '/api/runners';
const LABELS_PATH = '/runner-labels';

function policyUrl(): string {
  return `${API_BASE}${POLICY_PATH}`;
}

function runnersUrl(): string {
  return `${API_BASE}${RUNNERS_PATH}`;
}

function runnerUrl(runnerId: string): string {
  return `${runnersUrl()}/${encodeURIComponent(runnerId)}`;
}

function labelsUrl(scope: PlacementScope): string {
  const organization = `${API_BASE}/api/organizations/${encodeURIComponent(scope.organizationId)}`;
  return scope.kind === PlacementKind.Organization
    ? `${organization}${LABELS_PATH}`
    : `${organization}/workspaces/${encodeURIComponent(scope.workspaceId)}${LABELS_PATH}`;
}

async function failure(response: Response, action: string): Promise<ApiError> {
  const body: unknown = await response
    .clone()
    .json()
    .catch(() => undefined);
  const refusal = FleetRefusalSchema.safeParse(body);
  if (refusal.success) {
    return new FleetRefusal(refusal.data, response.status);
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

async function read(url: string, init: RequestInit, action: string): Promise<unknown> {
  return (await send(url, init, action)).json();
}

function patch(body: unknown): RequestInit {
  return { method: 'PATCH', body: JSON.stringify(body) };
}

function placementOf(scope: PlacementScope, body: unknown): Placement {
  return scope.kind === PlacementKind.Organization
    ? fromOrganization(parse(OrganizationRunnerLabelsResponseSchema, body))
    : fromWorkspace(parse(WorkspaceRunnerLabelsResponseSchema, body));
}

export const fleetApi = {
  async policy(): Promise<FleetPolicyResponse> {
    return parse(
      FleetPolicyResponseSchema,
      await read(policyUrl(), { method: 'GET' }, 'Failed to load the fleet policy')
    );
  },

  async updatePolicy(request: UpdateFleetPolicyRequest): Promise<FleetPolicyResponse> {
    return parse(
      FleetPolicyResponseSchema,
      await read(
        policyUrl(),
        patch(parse(UpdateFleetPolicyRequestSchema, request)),
        'Failed to save the fleet policy'
      )
    );
  },

  async runners(): Promise<RunnersResponse> {
    return parse(
      RunnersResponseSchema,
      await read(runnersUrl(), { method: 'GET' }, 'Failed to load the runners')
    );
  },

  async updateRunner(runnerId: string, request: UpdateRunnerRequest): Promise<RunnerResponse> {
    return parse(
      RunnerResponseSchema,
      await read(
        runnerUrl(runnerId),
        patch(parse(UpdateRunnerRequestSchema, request)),
        'Failed to change the runner'
      )
    );
  },

  async removeRunner(runnerId: string): Promise<void> {
    await send(runnerUrl(runnerId), { method: 'DELETE' }, 'Failed to remove the runner');
  },

  async labels(scope: PlacementScope): Promise<Placement> {
    return placementOf(
      scope,
      await read(labelsUrl(scope), { method: 'GET' }, 'Failed to load the runner labels')
    );
  },

  async updateLabels(
    scope: PlacementScope,
    request: UpdateRunnerLabelsRequest
  ): Promise<Placement> {
    return placementOf(
      scope,
      await read(
        labelsUrl(scope),
        patch(parse(UpdateRunnerLabelsRequestSchema, request)),
        'Failed to save the runner labels'
      )
    );
  },
};
