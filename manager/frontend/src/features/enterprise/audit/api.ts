import { ApiError } from '../../../api/ApiError';
import { API_BASE, client } from '../../../api/client';
import { parse } from '../../../validation';
import { AuditRefusal } from './AuditRefusal';
import { dispositionFilename, exportFilename } from './filename';
import { DOWNLOAD_INTERRUPTED } from './labels';
import {
  AuditRefusalSchema,
  AuditScopeKind,
  CreateDestinationRequestSchema,
  DestinationResponseSchema,
  DestinationsResponseSchema,
  ExportQuerySchema,
  RetentionResponseSchema,
  TestResponseSchema,
  UpdateDestinationRequestSchema,
  UpdateRetentionRequestSchema,
} from './schemas';
import type {
  AuditScope,
  CreateDestinationRequest,
  DestinationResponse,
  DestinationsResponse,
  ExportDownload,
  ExportQuery,
  RetentionResponse,
  TestResponse,
  UpdateDestinationRequest,
  UpdateRetentionRequest,
} from './types';

const DESTINATIONS_PATH = '/audit-destinations';

const RETENTION_PATH = '/audit-retention';

const EXPORTS_PATH = '/audit-exports';

const TESTS_PATH = '/tests';

const CONTENT_DISPOSITION = 'Content-Disposition';

function scopeUrl(scope: AuditScope, path: string): string {
  return scope.kind === AuditScopeKind.Organization
    ? `${API_BASE}/api/organizations/${encodeURIComponent(scope.organizationId)}${path}`
    : `${API_BASE}/api${path}`;
}

function destinationUrl(scope: AuditScope, destinationId: string, path = ''): string {
  return scopeUrl(scope, `${DESTINATIONS_PATH}/${encodeURIComponent(destinationId)}${path}`);
}

async function failure(response: Response, action: string): Promise<ApiError> {
  const body: unknown = await response
    .clone()
    .json()
    .catch(() => undefined);
  const refusal = AuditRefusalSchema.safeParse(body);
  if (refusal.success) {
    return new AuditRefusal(refusal.data, response.status);
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

function body(value: unknown): string {
  return JSON.stringify(value);
}

async function file(response: Response): Promise<Blob> {
  try {
    return await response.blob();
  } catch {
    throw new ApiError(DOWNLOAD_INTERRUPTED, response.status);
  }
}

export const auditApi = {
  async destinations(scope: AuditScope): Promise<DestinationsResponse> {
    const response = await send(
      scopeUrl(scope, DESTINATIONS_PATH),
      { method: 'GET' },
      'Failed to load the audit destinations'
    );
    return parse(DestinationsResponseSchema, await response.json());
  },

  async destination(scope: AuditScope, destinationId: string): Promise<DestinationResponse> {
    const response = await send(
      destinationUrl(scope, destinationId),
      { method: 'GET' },
      'Failed to load the audit destination'
    );
    return parse(DestinationResponseSchema, await response.json());
  },

  async createDestination(
    scope: AuditScope,
    request: CreateDestinationRequest
  ): Promise<DestinationResponse> {
    const response = await send(
      scopeUrl(scope, DESTINATIONS_PATH),
      { method: 'POST', body: body(parse(CreateDestinationRequestSchema, request)) },
      'Failed to add the audit destination'
    );
    return parse(DestinationResponseSchema, await response.json());
  },

  async updateDestination(
    scope: AuditScope,
    destinationId: string,
    request: UpdateDestinationRequest
  ): Promise<DestinationResponse> {
    const response = await send(
      destinationUrl(scope, destinationId),
      { method: 'PATCH', body: body(parse(UpdateDestinationRequestSchema, request)) },
      'Failed to save the audit destination'
    );
    return parse(DestinationResponseSchema, await response.json());
  },

  async deleteDestination(scope: AuditScope, destinationId: string): Promise<void> {
    await send(
      destinationUrl(scope, destinationId),
      { method: 'DELETE' },
      'Failed to delete the audit destination'
    );
  },

  async testDestination(scope: AuditScope, destinationId: string): Promise<TestResponse> {
    const response = await send(
      destinationUrl(scope, destinationId, TESTS_PATH),
      { method: 'POST' },
      'Failed to send a test event'
    );
    return parse(TestResponseSchema, await response.json());
  },

  async retention(scope: AuditScope): Promise<RetentionResponse> {
    const response = await send(
      scopeUrl(scope, RETENTION_PATH),
      { method: 'GET' },
      'Failed to load audit retention'
    );
    return parse(RetentionResponseSchema, await response.json());
  },

  async setRetention(
    scope: AuditScope,
    request: UpdateRetentionRequest
  ): Promise<RetentionResponse> {
    const response = await send(
      scopeUrl(scope, RETENTION_PATH),
      { method: 'PUT', body: body(parse(UpdateRetentionRequestSchema, request)) },
      'Failed to save audit retention'
    );
    return parse(RetentionResponseSchema, await response.json());
  },

  async download(scope: AuditScope, query: ExportQuery): Promise<ExportDownload> {
    const validated = parse(ExportQuerySchema, query);
    const search = new URLSearchParams(validated).toString();
    const response = await send(
      `${scopeUrl(scope, EXPORTS_PATH)}?${search}`,
      { method: 'GET' },
      'Failed to download the audit log'
    );
    return {
      blob: await file(response),
      filename: dispositionFilename(
        response.headers.get(CONTENT_DISPOSITION),
        exportFilename(scope, validated)
      ),
    };
  },
};
