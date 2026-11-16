import { ApiError } from '../../api/ApiError';
import { HttpStatus } from '../../api/HttpStatus';
import type { LicenseRefusalBody, LicenseRefusalCode } from './types';

export class LicenseRefusal extends ApiError {
  readonly code: LicenseRefusalCode;

  constructor(refusal: LicenseRefusalBody) {
    super(refusal.error, HttpStatus.BadRequest);
    this.name = 'LicenseRefusal';
    this.code = refusal.code;
  }
}
