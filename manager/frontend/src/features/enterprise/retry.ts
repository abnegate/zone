import { ApiError } from '../../api/ApiError';
import { HttpStatus } from '../../api/HttpStatus';

export const RETRY_LIMIT = 3;

export function refused(error: Error): boolean {
  return (
    error instanceof ApiError &&
    error.status >= HttpStatus.BadRequest &&
    error.status < HttpStatus.InternalServerError
  );
}

export function retryLoad(failures: number, error: Error): boolean {
  return !refused(error) && failures < RETRY_LIMIT;
}
