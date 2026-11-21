import type { Price } from './Price.ts';

export interface Site {
  readonly name: string;
  readonly domain: string;
  readonly repository: string;
  readonly checkoutURL: string;
  readonly contactEmail: string;
  readonly price: Price;
}
