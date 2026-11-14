import type { CreateMcpServerRequest, UpdateMcpServerRequest } from './types';

export const SubmissionKind = { Create: 'create', Update: 'update' } as const;

export type McpServerSubmission =
  | { kind: typeof SubmissionKind.Create; request: CreateMcpServerRequest }
  | { kind: typeof SubmissionKind.Update; request: UpdateMcpServerRequest };
