import type { Site } from './generator/Site.ts';

export const site: Site = {
  name: 'Zone',
  domain: 'zone.abnegate.dev',
  repository: 'https://github.com/abnegate/zone',
  checkoutURL: 'https://buy.polar.sh/polar_cl_MFvv7Q0eFuxGBzyPPNFGhhGtAPcfQGpgUnLEu14oZsz',
  contactEmail: 'sales@zone.abnegate.dev',
  price: {
    currency: 'USD',
    seatMonthly: 25,
    minimumSeats: 5,
  },
};

export const unconfirmed: ReadonlyArray<keyof Site> = ['contactEmail', 'domain'];
