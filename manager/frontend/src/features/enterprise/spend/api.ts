import { ApiError } from '../../../api/ApiError';
import { API_BASE, client } from '../../../api/client';
import { parse } from '../../../validation';
import { dispositionFilename } from '../audit/filename';
import type { ExportDownload } from '../audit/types';
import { DOWNLOAD_INTERRUPTED } from './labels';
import { SpendRefusal } from './SpendRefusal';
import {
  type Budget,
  BudgetSchema,
  type BudgetsResponse,
  BudgetsResponseSchema,
  type CreateBudgetRequest,
  CreateBudgetRequestSchema,
  type SpendQuery,
  SpendQuerySchema,
  SpendRefusalSchema,
  type SpendReportResponse,
  SpendReportResponseSchema,
  type UpdateBudgetRequest,
  UpdateBudgetRequestSchema,
} from './schemas';

const REPORTS_PATH = '/spend-reports';

const EXPORTS_PATH = '/spend-exports';

const BUDGETS_PATH = '/budgets';

const CONTENT_DISPOSITION = 'Content-Disposition';

const FILENAME_PREFIX = 'zone-spend';

const CSV_EXTENSION = 'csv';

function organizationUrl(organizationId: string, path: string): string {
  return `${API_BASE}/api/organizations/${encodeURIComponent(organizationId)}${path}`;
}

function budgetUrl(organizationId: string, budgetId: string): string {
  return organizationUrl(organizationId, `${BUDGETS_PATH}/${encodeURIComponent(budgetId)}`);
}

function search(query: SpendQuery): string {
  const validated = parse(SpendQuerySchema, query);
  const entries = Object.entries(validated).filter(
    (entry): entry is [string, string] => entry[1] !== undefined
  );
  return new URLSearchParams(entries).toString();
}

async function failure(response: Response, action: string): Promise<ApiError> {
  const body: unknown = await response
    .clone()
    .json()
    .catch(() => undefined);
  const refusal = SpendRefusalSchema.safeParse(body);
  if (refusal.success) {
    return new SpendRefusal(refusal.data, response.status);
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

async function file(response: Response): Promise<Blob> {
  try {
    return await response.blob();
  } catch {
    throw new ApiError(DOWNLOAD_INTERRUPTED, response.status);
  }
}

export function exportFilename(query: Pick<SpendQuery, 'from' | 'to'>): string {
  return `${FILENAME_PREFIX}-${query.from}-${query.to}.${CSV_EXTENSION}`;
}

export const spendApi = {
  async report(organizationId: string, query: SpendQuery): Promise<SpendReportResponse> {
    const response = await send(
      `${organizationUrl(organizationId, REPORTS_PATH)}?${search(query)}`,
      { method: 'GET' },
      'Failed to load spend'
    );
    return parse(SpendReportResponseSchema, await response.json());
  },

  async download(organizationId: string, query: SpendQuery): Promise<ExportDownload> {
    const response = await send(
      `${organizationUrl(organizationId, EXPORTS_PATH)}?${search(query)}`,
      { method: 'GET' },
      'Failed to download spend'
    );
    return {
      blob: await file(response),
      filename: dispositionFilename(
        response.headers.get(CONTENT_DISPOSITION),
        exportFilename(query)
      ),
    };
  },

  async budgets(organizationId: string): Promise<BudgetsResponse> {
    const response = await send(
      organizationUrl(organizationId, BUDGETS_PATH),
      { method: 'GET' },
      'Failed to load budgets'
    );
    return parse(BudgetsResponseSchema, await response.json());
  },

  async createBudget(organizationId: string, request: CreateBudgetRequest): Promise<Budget> {
    const response = await send(
      organizationUrl(organizationId, BUDGETS_PATH),
      { method: 'POST', body: JSON.stringify(parse(CreateBudgetRequestSchema, request)) },
      'Failed to add the budget'
    );
    return parse(BudgetSchema, await response.json());
  },

  async updateBudget(
    organizationId: string,
    budgetId: string,
    request: UpdateBudgetRequest
  ): Promise<Budget> {
    const response = await send(
      budgetUrl(organizationId, budgetId),
      { method: 'PATCH', body: JSON.stringify(parse(UpdateBudgetRequestSchema, request)) },
      'Failed to save the budget'
    );
    return parse(BudgetSchema, await response.json());
  },

  async deleteBudget(organizationId: string, budgetId: string): Promise<void> {
    await send(
      budgetUrl(organizationId, budgetId),
      { method: 'DELETE' },
      'Failed to delete the budget'
    );
  },
};
