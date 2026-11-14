import { ApiError } from '../../api/ApiError';
import { HttpStatus } from '../../api/HttpStatus';
import { RECONCILIATION_CODE, TAG_REFUSAL_CODES } from './schemas';
import type { Reconciliation, Refusal, RefusalCode, TagRefusal } from './types';

function refusesTag(refusal: Refusal): refusal is TagRefusal {
  return TAG_REFUSAL_CODES.includes(refusal.code);
}

export class ReleaseRefusal extends ApiError {
  readonly code: RefusalCode;
  readonly reconciliation: Reconciliation | null;
  readonly tagRefusal: TagRefusal | null;

  constructor(refusal: Refusal) {
    super(refusal.error, HttpStatus.Conflict);
    this.name = 'ReleaseRefusal';
    this.code = refusal.code;
    this.reconciliation = refusal.code === RECONCILIATION_CODE ? refusal : null;
    this.tagRefusal = refusesTag(refusal) ? refusal : null;
  }
}
