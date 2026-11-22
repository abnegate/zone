import { describe, expect, test } from 'bun:test';
import { site } from '../config.ts';
import { baseURLFrom, renderSite } from './build.ts';
import type { Page } from './Page.ts';
import { pages } from './pages.ts';
import { directoryOf, escapeHTML, fill, formatMoney, rootFor, siteValues } from './render.ts';

function pageAt(output: string, linksFromBasePath = false): Page {
  return {
    template: 'home.html',
    output,
    title: 'Title',
    description: 'Description',
    indexed: true,
    linksFromBasePath,
  };
}

describe('fill', () => {
  test('escapes values and keeps trusted markup', () => {
    const filled = fill('<p>{{ name }}</p>{{ body }}', { name: '<b>&"\'' }, { body: '<em>x</em>' });
    expect(filled).toBe('<p>&lt;b&gt;&amp;&quot;&#39;</p><em>x</em>');
  });

  test('refuses a token it has no value for', () => {
    expect(() => fill('{{ price }} {{ missing }}', { price: '1' })).toThrow('missing');
  });

  test('does not expand tokens inside substituted markup', () => {
    expect(fill('{{ body }}', {}, { body: '{{ body }}' })).toBe('{{ body }}');
  });
});

describe('links', () => {
  test('roots relative links at the page depth', () => {
    expect(rootFor(pageAt('index.html'), '/')).toBe('./');
    expect(rootFor(pageAt('pricing/index.html'), '/')).toBe('../');
    expect(rootFor(pageAt('a/b/index.html'), '/')).toBe('../../');
  });

  test('roots the not found page at the base path', () => {
    expect(rootFor(pageAt('404.html', true), '/zone')).toBe('/zone/');
    expect(rootFor(pageAt('404.html', true), '/')).toBe('/');
  });

  test('links to directories rather than index files', () => {
    expect(directoryOf('index.html')).toBe('');
    expect(directoryOf('pricing/index.html')).toBe('pricing/');
    expect(directoryOf('404.html')).toBe('404.html');
  });
});

describe('price', () => {
  test('formats whole and fractional amounts', () => {
    expect(formatMoney(25, 'USD')).toBe('$25');
    expect(formatMoney(1500, 'USD')).toBe('$1,500');
    expect(formatMoney(24.5, 'USD')).toBe('$24.50');
  });

  test('shows the yearly price and what it saves over twelve months', () => {
    const values = siteValues({
      ...site,
      price: { currency: 'USD', seatMonthly: 20, seatYearly: 200 },
    });
    expect(values.seatMonthly).toBe('$20');
    expect(values.seatYearly).toBe('$200');
    expect(values.yearlySaving).toBe('$40');
    expect(values.contactHref).toBe(`mailto:${site.contactEmail}`);
  });

  test('escapes HTML special characters', () => {
    expect(escapeHTML(`<a href="x">'&'</a>`)).toBe(
      '&lt;a href=&quot;x&quot;&gt;&#39;&amp;&#39;&lt;/a&gt;'
    );
  });
});

describe('site', () => {
  const baseURL = baseURLFrom('https://bitecodehq.github.io/zone');
  const rendered = renderSite(baseURL);

  test('renders every page', () => {
    expect([...rendered.keys()].sort()).toEqual(pages.map((page) => page.output).sort());
  });

  for (const page of pages) {
    describe(page.output, () => {
      const html = rendered.get(page.output) ?? '';

      test('leaves no template tokens behind', () => {
        expect(html).not.toMatch(/\{\{|\}\}/);
      });

      test('has one top-level heading, a language and a main landmark', () => {
        expect(html.match(/<h1[\s>]/g)?.length).toBe(1);
        expect(html).toContain('<html lang="en">');
        expect(html).toContain('<main id="main"');
      });

      test('loads no third-party scripts', () => {
        expect(html).not.toMatch(/<script/i);
      });

      if (page.indexed) {
        test('names its canonical address under the deployment base URL', () => {
          const canonical = new URL(directoryOf(page.output), baseURL).href;
          expect(html).toContain(`<link rel="canonical" href="${canonical}">`);
          expect(canonical.startsWith('https://bitecodehq.github.io/zone/')).toBe(true);
        });
      }

      test('is only indexed when public', () => {
        expect(html).toContain(page.indexed ? 'index, follow' : 'noindex');
      });
    });
  }

  test('sends every buy button to the checkout', () => {
    const pricing = rendered.get('pricing/index.html') ?? '';
    const buttons = pricing.match(/<a [^>]*>Buy Zone Enterprise<\/a>/g) ?? [];
    expect(buttons.length).toBeGreaterThan(0);
    for (const button of buttons) {
      expect(button).toContain(`href="${escapeHTML(site.checkoutURL)}"`);
    }
  });

  test('marks the current page in the navigation', () => {
    expect(rendered.get('pricing/index.html')).toContain(
      '<a href="../pricing/" aria-current="page">'
    );
    expect(rendered.get('index.html')).not.toContain('aria-current');
  });

  test('roots the not found page at the base path', () => {
    expect(rendered.get('404.html')).toContain('href="/zone/static/style.css"');
  });
});
