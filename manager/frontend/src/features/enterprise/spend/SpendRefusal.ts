import { ApiError } from '../../../api/ApiError';
import type { SpendRefusalBody, SpendRefusalCode } from './schemas';

export class SpendRefusal extends ApiError {
  readonly code: SpendRefusalCode;

  constructor(refusal: SpendRefusalBody, status: number) {
    super(refusal.error, status);
    this.name = 'SpendRefusal';
    this.code = refusal.code;
  }
}
