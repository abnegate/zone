import { describe, expect, test } from 'bun:test';
import { site } from '../config.ts';
import { baseURLFrom, fragmentName, renderSite } from './build.ts';
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

describe('fragments', () => {
  test('names each fragment token after its file in camel case', () => {
    expect(fragmentName('contact-form.html')).toBe('contactForm');
    expect(fragmentName('note.html')).toBe('note');
  });
});

describe('price', () => {
  test('formats whole and fractional amounts', () => {
    expect(formatMoney(25, 'USD')).toBe('$25');
    expect(formatMoney(1500, 'USD')).toBe('$1,500');
    expect(formatMoney(24.5, 'USD')).toBe('$24.50');
  });

  test('shows each plan with its yearly price and what it saves over twelve months', () => {
    const values = siteValues({
      ...site,
      currency: 'USD',
      plans: {
        community: { memberLimit: 5 },
        team: { seatMonthly: 12, seatYearly: 120, checkoutURL: 'https://example.com/team' },
        enterprise: {
          seatMonthly: 24.5,
          seatYearly: 250,
          checkoutURL: 'https://example.com/enterprise',
        },
      },
    });
    expect(values.communityMemberLimit).toBe('5');
    expect(values.teamSeatMonthly).toBe('$12');
    expect(values.teamSeatYearly).toBe('$120');
    expect(values.teamYearlySaving).toBe('$24');
    expect(values.teamCheckoutURL).toBe('https://example.com/team');
    expect(values.enterpriseSeatMonthly).toBe('$24.50');
    expect(values.enterpriseSeatYearly).toBe('$250');
    expect(values.enterpriseYearlySaving).toBe('$44');
    expect(values.enterpriseCheckoutURL).toBe('https://example.com/enterprise');
    expect(values.contactFormURL).toBe(site.contactFormURL);
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

      test('never opens an email client', () => {
        expect(html).not.toContain('mailto:');
      });

      test('does not claim Community is uncapped', () => {
        expect(html).not.toMatch(/no (seat|member)[^.<]*caps|no caps/i);
      });
    });
  }

  const paidPlans = [
    { label: 'Team', plan: site.plans.team, other: site.plans.enterprise },
    { label: 'Enterprise', plan: site.plans.enterprise, other: site.plans.team },
  ];

  for (const { label, plan, other } of paidPlans) {
    test(`sends every Buy Zone ${label} button to the ${label} checkout`, () => {
      const pricing = rendered.get('pricing/index.html') ?? '';
      const buttons = pricing.match(new RegExp(`<a [^>]*>Buy Zone ${label}</a>`, 'g')) ?? [];
      expect(buttons.length).toBeGreaterThan(0);
      for (const button of buttons) {
        expect(button).toContain(`href="${escapeHTML(plan.checkoutURL)}"`);
        expect(button).not.toContain(escapeHTML(other.checkoutURL));
      }
    });

    test(`prices ${label} from the config`, () => {
      const pricing = rendered.get('pricing/index.html') ?? '';
      const card = pricing.match(
        new RegExp(
          `<article [^>]*aria-labelledby="${label.toLowerCase()}-heading"[\\s\\S]*?</article>`
        )
      )?.[0];
      expect(card).toBeDefined();
      const monthly = formatMoney(plan.seatMonthly, site.currency);
      const yearly = formatMoney(plan.seatYearly, site.currency);
      const saving = formatMoney(plan.seatMonthly * 12 - plan.seatYearly, site.currency);
      expect(card).toContain(`<span class="amount">${monthly}</span>`);
      expect(card).toContain(`${yearly} per seat per year`);
      expect(card).toContain(`saving ${saving} a seat`);
    });
  }

  test('states the Community member limit from the config', () => {
    const limit = `up to ${site.plans.community.memberLimit} active members`;
    expect(rendered.get('pricing/index.html')).toContain(limit);
    expect(rendered.get('index.html')).toContain(limit);
    expect(rendered.get('licensing/index.html')).toContain(limit);
  });

  test('links every Contact to the contact page from any depth', () => {
    expect(rendered.get('index.html')).toContain('<a href="./contact/">Contact</a>');
    expect(rendered.get('pricing/index.html')).toContain('<a href="../contact/">Contact</a>');
    expect(rendered.get('404.html')).toContain('<a href="/zone/contact/">Contact</a>');
  });

  for (const output of ['contact/index.html', 'pricing/index.html']) {
    test(`shows the contact form inline on ${output}`, () => {
      const html = rendered.get(output) ?? '';
      const form = html.match(/<form class="contact-form"[\s\S]*?<\/form>/)?.[0];
      expect(form).toBeDefined();
      expect(form).toContain(`action="${escapeHTML(site.contactFormURL)}" method="post"`);
      expect(form).toContain(
        '<input type="hidden" name="redirect" value="https://bitecodehq.github.io/zone/contact/sent/">'
      );
      for (const field of ['name', 'email', 'message']) {
        expect(form).toMatch(new RegExp(`name="${field}"[^>]*required`));
      }
      expect(form).toMatch(/name="website"[^>]*tabindex="-1"/);
    });
  }

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
