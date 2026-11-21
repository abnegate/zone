import { ApiError } from '../../../api/ApiError';
import type { RoleCode } from './schemas';
import type { RoleRefusalBody } from './types';

export class RoleRefusal extends ApiError {
  readonly code: RoleCode;
  readonly members: number | undefined;

  constructor(refusal: RoleRefusalBody, status: number) {
    super(refusal.error, status);
    this.name = 'RoleRefusal';
    this.code = refusal.code;
    this.members = refusal.members;
  }
}
