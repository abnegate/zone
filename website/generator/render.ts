import type { Page } from './Page.ts';
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

export function siteValues(site: Site): Record<string, string> {
  const { currency, seatMonthly, seatYearly } = site.price;
  return {
    siteName: site.name,
    seatMonthly: formatMoney(seatMonthly, currency),
    seatYearly: formatMoney(seatYearly, currency),
    yearlySaving: formatMoney(seatMonthly * 12 - seatYearly, currency),
    currency,
    checkoutURL: site.checkoutURL,
    contactEmail: site.contactEmail,
    contactHref: `mailto:${site.contactEmail}`,
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
  site: Site,
  baseURL: URL
): string {
  const root = rootFor(page, baseURL.pathname);
  const values = {
    ...siteValues(site),
    root,
    title: page.title,
    description: page.description,
    robots: page.indexed ? 'index, follow' : 'noindex',
  };
  const canonicalLink = page.indexed
    ? `<link rel="canonical" href="${escapeHTML(new URL(directoryOf(page.output), baseURL).href)}">`
    : '';
  const content = fill(template, values);
  return fill(layout, values, {
    content,
    canonicalLink,
    navigation: renderNavigation(page, pages, root),
  });
}
