import type { Plans } from './Plans.ts';

export interface Site {
  readonly name: string;
  readonly domain: string;
  readonly repository: string;
  readonly contactEmail: string;
  readonly currency: string;
  readonly plans: Plans;
}
