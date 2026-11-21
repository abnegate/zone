import type { SpendLicense } from './schemas';

type Licensed = Pick<SpendLicense, 'entitled' | 'enforced'>;

export function unlicensed(response: Licensed): boolean {
  return !response.enforced;
}

export function lapsed(response: Licensed): boolean {
  return response.enforced && !response.entitled;
}
