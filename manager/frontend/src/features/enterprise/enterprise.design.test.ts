import { describe, expect, it } from 'bun:test';
import { join } from 'node:path';
import { media, read, rule } from '../../test/css';

const css = read(join(import.meta.dir, 'components', 'LicenseSection.css'));

const PHONE = '(max-width: 768px)';

describe('license section surfaces', () => {
  it('draws only from the design tokens', () => {
    const variables = [...css.matchAll(/var\((--[\w-]+)/g)].map((match) => match[1]);
    expect(variables.length).toBeGreaterThan(0);
    for (const variable of variables) {
      expect(variable).toStartWith('--ui-');
    }
  });

  it('has no color literals', () => {
    expect(css).not.toMatch(/#[0-9a-f]{3,8}\b/i);
    expect(css).not.toMatch(/\b(?:rgb|rgba|hsl|hsla|oklch)\(/i);
  });

  it('sets the pasted key in the mono face and breaks it anywhere', () => {
    const input = rule(css, '.license-key-input');
    expect(input).toContain('font-family: var(--ui-font-mono)');
    expect(input).toContain('word-break: break-all');
  });

  it('spaces the key field from its actions with the form gap alone', () => {
    expect(rule(css, '.license-key-form')).toContain('gap: var(--ui-space-3)');
    expect(rule(css, '.license-key-form .form-group')).toContain('margin-bottom: 0');
  });

  it('stacks the unverified reason under its headline with no paragraph margins', () => {
    const warning = rule(css, '.license-unverified');
    expect(warning).toContain('flex-direction: column');
    expect(warning).toContain('gap: var(--ui-space-1)');
    expect(rule(css, '.license-unverified p')).toContain('margin: 0');
  });

  it('sets the renewal outcome beside its date and wraps it under on a narrow column', () => {
    const attempt = rule(css, '.license-renewal-attempt');
    expect(attempt).toContain('display: flex');
    expect(attempt).toContain('flex-wrap: wrap');
    expect(attempt).toContain('align-items: center');
    expect(attempt).toContain('gap: var(--ui-space-2)');
  });

  it('lets every column shrink below its content', () => {
    for (const selector of [
      '.license-section',
      '.license-summary',
      '.license-summary-header',
      '.license-edition',
      '.license-detail',
      '.license-key-form',
    ]) {
      expect(rule(css, selector), selector).toContain('min-width: 0');
    }
    expect(rule(css, '.license-details')).toContain(
      'grid-template-columns: repeat(2, minmax(0, 1fr))'
    );
    expect(rule(css, '.license-detail dd')).toContain('overflow-wrap: anywhere');
  });

  it('stacks the details and wraps the header and actions on a phone so nothing scrolls sideways', () => {
    const phone = media(css, PHONE);
    expect(rule(phone, '.license-details')).toContain('grid-template-columns: minmax(0, 1fr)');
    expect(rule(phone, '.license-summary-header')).toContain('flex-wrap: wrap');
    expect(rule(phone, '.license-key-actions')).toContain('flex-wrap: wrap');
  });
});

describe('enterprise switch surfaces', () => {
  const switchCss = read(join(import.meta.dir, 'components', 'Switch.css'));

  it('draws only from the design tokens and has no color literals', () => {
    const variables = [...switchCss.matchAll(/var\((--[\w-]+)/g)].map((match) => match[1]);
    expect(variables.length).toBeGreaterThan(0);
    for (const variable of variables) {
      expect(variable).toStartWith('--ui-');
    }
    expect(switchCss).not.toMatch(/#[0-9a-f]{3,8}\b/i);
    expect(switchCss).not.toMatch(/\b(?:rgb|rgba|hsl|hsla|oklch|color-mix)\(/i);
  });

  it('lets the switch and its text shrink below their content', () => {
    for (const selector of ['.enterprise-switch', '.enterprise-switch-text']) {
      expect(rule(switchCss, selector), selector).toContain('min-width: 0');
    }
  });

  it('colors the switch from the accent only when it is on', () => {
    expect(rule(switchCss, '.enterprise-switch-control')).toContain(
      'background-color: var(--ui-bg-active)'
    );
    expect(rule(switchCss, ".enterprise-switch-control[aria-checked='true']")).toContain(
      'background-color: var(--ui-accent)'
    );
    expect(
      rule(switchCss, ".enterprise-switch-control[aria-checked='true'] .enterprise-switch-thumb")
    ).toContain('transform: translateX(0.875rem)');
    expect(rule(switchCss, '.enterprise-switch-control:focus-visible')).toContain(
      'outline: 2px solid var(--ui-accent)'
    );
    expect(rule(switchCss, '.enterprise-switch-control:disabled')).toContain('cursor: not-allowed');
  });
});

describe('plan upgrade notice surfaces', () => {
  it('stacks the heading, message and link and lets them shrink', () => {
    const notice = rule(css, '.plan-notice');
    expect(notice).toContain('flex-direction: column');
    expect(notice).toContain('min-width: 0');
    expect(rule(css, '.plan-notice-title')).toContain('margin: 0');
    expect(rule(css, '.plan-notice p')).toContain('margin: 0');
  });
});

describe('member cap banner surfaces', () => {
  const bannerCss = read(join(import.meta.dir, 'components', 'MemberCapBanner.css'));

  it('draws only from the design tokens and has no color literals', () => {
    const variables = [...bannerCss.matchAll(/var\((--[\w-]+)/g)].map((match) => match[1]);
    expect(variables.length).toBeGreaterThan(0);
    for (const variable of variables) {
      expect(variable).toStartWith('--ui-');
    }
    expect(bannerCss).not.toMatch(/#[0-9a-f]{3,8}\b/i);
    expect(bannerCss).not.toMatch(/\b(?:rgb|rgba|hsl|hsla|oklch|color-mix)\(/i);
  });

  it('wraps its text and actions and lets them shrink so nothing scrolls sideways', () => {
    for (const selector of [
      '.member-cap-banner',
      '.member-cap-banner-text',
      '.member-cap-banner-actions',
    ]) {
      expect(rule(bannerCss, selector), selector).toContain('min-width: 0');
    }
    expect(rule(bannerCss, '.member-cap-banner')).toContain('flex-wrap: wrap');
    expect(rule(bannerCss, '.member-cap-banner-actions')).toContain('flex-wrap: wrap');
    expect(rule(media(bannerCss, PHONE), '.member-cap-banner-text')).toContain('flex-basis: 100%');
  });

  it('keeps a workspace page filling the rest of the viewport under the banner', () => {
    const page = rule(bannerCss, '.main-content:has(> .member-cap-banner) .page--workspace');
    expect(page).toContain('flex: 1');
    expect(page).toContain('height: auto');
    expect(page).toContain('min-height: 0');
  });
});
