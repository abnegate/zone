import { site } from '../config.ts';
import type { Page } from './Page.ts';

export const pages: ReadonlyArray<Page> = [
  {
    template: 'home.html',
    output: 'index.html',
    title: 'Zone: the self-hosted AI workspace',
    description:
      'Zone runs chat, coding agents and local models on hardware you own. Free and MIT-licensed, with Team and Enterprise plans for companies.',
    indexed: true,
    linksFromBasePath: false,
  },
  {
    template: 'pricing.html',
    output: 'pricing/index.html',
    title: 'Pricing · Zone',
    description: `Zone Community is free for up to ${site.plans.community.memberLimit} active members per server. Zone Team adds unlimited members, spend reports and budgets. Zone Enterprise adds SSO, SCIM provisioning, agent and model policy, custom roles and audit export. Per seat, monthly or yearly.`,
    navigationLabel: 'Pricing',
    indexed: true,
    linksFromBasePath: false,
  },
  {
    template: 'licensing.html',
    output: 'licensing/index.html',
    title: 'Licensing · Zone',
    description:
      'Zone is MIT-licensed, apart from the Team and Enterprise code, which is source-available under the Zone Enterprise License.',
    navigationLabel: 'Licensing',
    indexed: true,
    linksFromBasePath: false,
  },
  {
    template: 'thanks.html',
    output: 'thanks/index.html',
    title: 'Thank you · Zone',
    description: 'Your Zone license key is on its way to your billing contact.',
    indexed: false,
    linksFromBasePath: false,
  },
  {
    template: 'contact.html',
    output: 'contact/index.html',
    title: 'Contact · Zone',
    description:
      'Ask about Zone plans, invoicing, licensing or a large rollout. A person replies by email.',
    indexed: true,
    linksFromBasePath: false,
  },
  {
    template: 'contact-sent.html',
    output: 'contact/sent/index.html',
    title: 'Message sent · Zone',
    description: 'Your message to the Zone team is on its way.',
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
