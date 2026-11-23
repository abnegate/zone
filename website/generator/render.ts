import type { Page } from './Page.ts';
import type { Plan } from './Plan.ts';
import type { Site } from './Site.ts';

const tokenPattern = /\{\{\s*([A-Za-z]+)\s*\}\}/g;

const htmlEscapes: Readonly<Record<string, string>> = {
  '&': '&amp;',
  '<': '&lt;',
  '>': '&gt;',
  '"': '&quot;',
  "'": '&#39;',
};

export function escapeHTML(value: string): string {
  return value.replace(/[&<>"']/g, (character) => htmlEscapes[character] ?? character);
}

export function fill(
  template: string,
  values: Readonly<Record<string, string>>,
  trusted: Readonly<Record<string, string>> = {}
): string {
  const unknown = new Set<string>();
  const filled = template.replace(tokenPattern, (token: string, name: string) => {
    const markup = trusted[name];
    if (markup !== undefined) {
      return markup;
    }
    const value = values[name];
    if (value !== undefined) {
      return escapeHTML(value);
    }
    unknown.add(name);
    return token;
  });
  if (unknown.size > 0) {
    throw new Error(`Unknown template tokens: ${[...unknown].join(', ')}`);
  }
  return filled;
}

export function formatMoney(amount: number, currency: string): string {
  const fractionDigits = Number.isInteger(amount) ? 0 : 2;
  return new Intl.NumberFormat('en-US', {
    style: 'currency',
    currency,
    minimumFractionDigits: fractionDigits,
    maximumFractionDigits: fractionDigits,
  }).format(amount);
}

export function directoryOf(output: string): string {
  return output.replace(/(^|\/)index\.html$/, '$1');
}

export function rootFor(page: Page, basePath: string): string {
  if (page.linksFromBasePath) {
    return basePath.endsWith('/') ? basePath : `${basePath}/`;
  }
  const depth = page.output.split('/').length - 1;
  return depth === 0 ? './' : '../'.repeat(depth);
}

export function planValues(name: string, plan: Plan, currency: string): Record<string, string> {
  return {
    [`${name}SeatMonthly`]: formatMoney(plan.seatMonthly, currency),
    [`${name}SeatYearly`]: formatMoney(plan.seatYearly, currency),
    [`${name}YearlySaving`]: formatMoney(plan.seatMonthly * 12 - plan.seatYearly, currency),
    [`${name}CheckoutURL`]: plan.checkoutURL,
  };
}

export function siteValues(site: Site): Record<string, string> {
  const { currency, plans } = site;
  return {
    siteName: site.name,
    ...planValues('team', plans.team, currency),
    ...planValues('enterprise', plans.enterprise, currency),
    communityMemberLimit: String(plans.community.memberLimit),
    currency,
    contactEmail: site.contactEmail,
    contactFormURL: site.contactFormURL,
    repository: site.repository,
    domain: site.domain,
  };
}

export function renderNavigation(page: Page, pages: ReadonlyArray<Page>, root: string): string {
  return pages
    .flatMap((target) => {
      if (target.navigationLabel === undefined) {
        return [];
      }
      const href = escapeHTML(root + directoryOf(target.output));
      const current = target.output === page.output ? ' aria-current="page"' : '';
      return [`<li><a href="${href}"${current}>${escapeHTML(target.navigationLabel)}</a></li>`];
    })
    .join('');
}

export function renderPage(
  page: Page,
  pages: ReadonlyArray<Page>,
  layout: string,
  template: string,
  fragments: Readonly<Record<string, string>>,
  site: Site,
  baseURL: URL
): string {
  const root = rootFor(page, baseURL.pathname);
  const values = {
    ...siteValues(site),
    root,
    contactHref: `${root}contact/`,
    contactSentURL: new URL('contact/sent/', baseURL).href,
    title: page.title,
    description: page.description,
    robots: page.indexed ? 'index, follow' : 'noindex',
  };
  const filledFragments = Object.fromEntries(
    Object.entries(fragments).map(([name, fragment]) => [name, fill(fragment, values)])
  );
  const canonicalLink = page.indexed
    ? `<link rel="canonical" href="${escapeHTML(new URL(directoryOf(page.output), baseURL).href)}">`
    : '';
  const content = fill(template, values, filledFragments);
  return fill(layout, values, {
    content,
    canonicalLink,
    navigation: renderNavigation(page, pages, root),
  });
}
