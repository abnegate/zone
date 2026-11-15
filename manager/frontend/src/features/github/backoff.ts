import { ApiError } from '../../api/ApiError';
import { HttpStatus } from '../../api/HttpStatus';

export const GATEWAY_BACKOFF_MS = [10_000, 20_000, 40_000, 80_000, 120_000] as const;

export const RETRY_LIMIT = 3;

const RETRY_BASE_MS = 1_000;

const RETRY_CEILING_MS = 30_000;

const FINAL_STATUSES: readonly number[] = [
  HttpStatus.BadRequest,
  HttpStatus.Forbidden,
  HttpStatus.NotFound,
  HttpStatus.Conflict,
];

export function notFound(error: unknown): boolean {
  return error instanceof ApiError && error.status === HttpStatus.NotFound;
}

export function gatewayFailed(error: unknown): boolean {
  return error instanceof ApiError && error.status === HttpStatus.BadGateway;
}

export function gatewayBackoff(failures: number): number {
  const index = Math.min(Math.max(failures, 1), GATEWAY_BACKOFF_MS.length) - 1;
  return GATEWAY_BACKOFF_MS[index];
}

function final(error: Error): boolean {
  return error instanceof ApiError && FINAL_STATUSES.includes(error.status);
}

export function retryRead(failures: number, error: Error): boolean {
  return !final(error) && failures < RETRY_LIMIT;
}

export function retryPolled(failures: number, error: Error): boolean {
  return gatewayFailed(error) || retryRead(failures, error);
}

export function retryDelay(failures: number, error: Error): number {
  return gatewayFailed(error)
    ? gatewayBackoff(failures + 1)
    : Math.min(RETRY_BASE_MS * 2 ** failures, RETRY_CEILING_MS);
}
