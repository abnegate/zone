import { CheckStateSchema } from '../schemas';
import type { CheckDetail, CheckState, PullRequest } from '../types';

export interface FailedCheck {
  name: string;
  state: CheckState;
  url: string | null;
  jobId: number | null;
}

function failed(check: CheckDetail): boolean {
  return check.state === CheckStateSchema.enum.failure;
}

export function failedChecks(pullRequest: Pick<PullRequest, 'check_details'>): FailedCheck[] {
  return pullRequest.check_details.filter(failed).map((check) => ({
    name: check.name,
    state: check.state,
    url: check.url,
    jobId: check.id,
  }));
}
