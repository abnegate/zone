import { ApiError } from '../../../api/ApiError';
import type { PolicyCode, PolicyRefusalBody } from './types';

export class PolicyRefusal extends ApiError {
  readonly code: PolicyCode;

  constructor(refusal: PolicyRefusalBody, status: number) {
    super(refusal.error, status);
    this.name = 'PolicyRefusal';
    this.code = refusal.code;
  }
}
