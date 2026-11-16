import { ApiError } from '../../../../api/ApiError';
import { HttpStatus } from '../../../../api/HttpStatus';

export const POLICY_RETRY_LIMIT = 3;

function refused(error: Error): boolean {
  return (
    error instanceof ApiError &&
    error.status >= HttpStatus.BadRequest &&
    error.status < HttpStatus.InternalServerError
  );
}

export function retryLoad(failures: number, error: Error): boolean {
  return !refused(error) && failures < POLICY_RETRY_LIMIT;
}
