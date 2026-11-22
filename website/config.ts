import type { Site } from './generator/Site.ts';

export const site: Site = {
  name: 'Zone',
  domain: 'zone.bitecode.ltd',
  repository: 'https://github.com/bitecodehq/zone',
  checkoutURL: 'https://buy.polar.sh/polar_cl_MFvv7Q0eFuxGBzyPPNFGhhGtAPcfQGpgUnLEu14oZsz',
  contactEmail: 'zone@bitecode.ltd',
  price: {
    currency: 'USD',
    seatMonthly: 20,
    seatYearly: 200,
  },
};

export const unconfirmed: ReadonlyArray<keyof Site> = [];
