import { ApiError } from '../../api/ApiError';
import { HttpStatus } from '../../api/HttpStatus';
import type { Blocker, Refusal, RefusalCode } from './types';

export class PullRequestRefusal extends ApiError {
  readonly code: RefusalCode;
  readonly currentHead: string | null;
  readonly blockers: readonly Blocker[];

  constructor(refusal: Refusal) {
    super(refusal.error, HttpStatus.Conflict);
    this.name = 'PullRequestRefusal';
    this.code = refusal.code;
    this.currentHead = refusal.current_head ?? null;
    this.blockers = refusal.blockers ?? [];
  }
}
