import { ApiError } from '../../api/ApiError';
import { HttpStatus } from '../../api/HttpStatus';

export const NO_WORKSPACE = 'No workspace selected';

export function errorMessage(error: unknown, fallback: string): string | null {
  if (!error) return null;
  return error instanceof Error ? error.message : fallback;
}

export function outcomeUnknown(error: unknown): boolean {
  return error instanceof ApiError && error.status === HttpStatus.InternalServerError;
}
