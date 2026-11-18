import { ApiError } from '../../../api/ApiError';
import type { SsoAdminCode, SsoRefusalBody } from './types';

export class SsoRefusal extends ApiError {
  readonly code: SsoAdminCode;

  constructor(refusal: SsoRefusalBody, status: number) {
    super(refusal.error, status);
    this.name = 'SsoRefusal';
    this.code = refusal.code;
  }
}
