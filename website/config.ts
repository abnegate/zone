import type { Site } from './generator/Site.ts';

export const site: Site = {
  name: 'Zone',
  domain: 'zone.abnegate.dev',
  repository: 'https://github.com/abnegate/zone',
  checkoutURL: 'https://buy.polar.sh/REPLACE_WITH_POLAR_CHECKOUT_LINK',
  contactEmail: 'sales@zone.abnegate.dev',
  price: {
    currency: 'USD',
    seatMonthly: 25,
    minimumSeats: 5,
  },
};

export const unconfirmed: ReadonlyArray<keyof Site> = [
  'checkoutURL',
  'contactEmail',
  'domain',
  'price',
];
