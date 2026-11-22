import { ApiError } from '../../../api/ApiError';
import { FleetCode } from './schemas';
import type { FleetAdminCode, FleetRefusalBody } from './types';

export class FleetRefusal extends ApiError {
  readonly code: FleetAdminCode;

  constructor(refusal: FleetRefusalBody, status: number) {
    super(refusal.error, status);
    this.name = 'FleetRefusal';
    this.code = refusal.code;
  }

  get leavesNoRunner(): boolean {
    return this.code === FleetCode.NoRunnerLeft;
  }
}
