import { ApiError } from '../../../api/ApiError';
import type { RoleCode } from './schemas';
import type { RoleRefusalBody } from './types';

export class RoleRefusal extends ApiError {
  declare readonly code: RoleCode;
  readonly members: number | undefined;

  constructor(refusal: RoleRefusalBody, status: number) {
    super(refusal.error, status, refusal.code);
    this.name = 'RoleRefusal';
    this.members = refusal.members;
  }
}
