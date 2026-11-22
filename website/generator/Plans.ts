import type { Community } from './Community.ts';
import type { Plan } from './Plan.ts';

export interface Plans {
  readonly community: Community;
  readonly team: Plan;
  readonly enterprise: Plan;
}
