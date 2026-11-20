import type { Page } from './Page.ts';

export const pages: ReadonlyArray<Page> = [
  {
    template: 'home.html',
    output: 'index.html',
    title: 'Zone: the self-hosted AI workspace',
    description:
      'Zone runs chat, coding agents and local models on hardware you own. Free and MIT-licensed, with Zone Enterprise for companies.',
    indexed: true,
    linksFromBasePath: false,
  },
  {
    template: 'pricing.html',
    output: 'pricing/index.html',
    title: 'Pricing · Zone',
    description:
      'Zone Community is free with no caps. Zone Enterprise adds SSO, agent and model policy and audit export, billed annually per seat.',
    navigationLabel: 'Pricing',
    indexed: true,
    linksFromBasePath: false,
  },
  {
    template: 'licensing.html',
    output: 'licensing/index.html',
    title: 'Licensing · Zone',
    description:
      'Zone is MIT-licensed, apart from the Enterprise code, which is source-available under the Zone Enterprise License.',
    navigationLabel: 'Licensing',
    indexed: true,
    linksFromBasePath: false,
  },
  {
    template: 'thanks.html',
    output: 'thanks/index.html',
    title: 'Thank you · Zone',
    description: 'Your Zone Enterprise license key is on its way to your billing contact.',
    indexed: false,
    linksFromBasePath: false,
  },
  {
    template: 'not-found.html',
    output: '404.html',
    title: 'Page not found · Zone',
    description: 'This page does not exist.',
    indexed: false,
    linksFromBasePath: true,
  },
];
