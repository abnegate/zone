import { ApiError } from '../../../api/ApiError';
import type { ScimCode } from './schemas';
import type { ScimRefusalBody } from './types';

export class ScimRefusal extends ApiError {
  declare readonly code: ScimCode;

  constructor(refusal: ScimRefusalBody, status: number) {
    super(refusal.error, status, refusal.code);
    this.name = 'ScimRefusal';
  }
}
