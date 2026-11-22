import type { Site } from './generator/Site.ts';

export const site: Site = {
  name: 'Zone',
  domain: 'zone.bitecode.ltd',
  repository: 'https://github.com/bitecodehq/zone',
  contactEmail: 'zone@bitecode.ltd',
  currency: 'USD',
  plans: {
    community: {
      memberLimit: 3,
    },
    team: {
      seatMonthly: 10,
      seatYearly: 100,
      checkoutURL: 'https://buy.polar.sh/polar_cl_zVuUr7Jxh34vgTPeJUFHHuhbSIif1lpiul0Qm1ba7BP',
    },
    enterprise: {
      seatMonthly: 20,
      seatYearly: 200,
      checkoutURL: 'https://buy.polar.sh/polar_cl_MFvv7Q0eFuxGBzyPPNFGhhGtAPcfQGpgUnLEu14oZsz',
    },
  },
};

export const unconfirmed: ReadonlyArray<keyof Site> = [];
