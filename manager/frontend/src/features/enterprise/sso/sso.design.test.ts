import { describe, expect, it } from 'bun:test';
import { join } from 'node:path';
import { media, read, rule } from '../../../test/css';

const css = read(join(import.meta.dir, 'components', 'SsoSection.css'));

const PHONE = '(max-width: 768px)';

describe('single sign-on section surfaces', () => {
  it('draws only from the design tokens', () => {
    const variables = [...css.matchAll(/var\((--[\w-]+)/g)].map((match) => match[1]);
    expect(variables.length).toBeGreaterThan(0);
    for (const variable of variables) {
      expect(variable).toStartWith('--ui-');
    }
  });

  it('has no color literals', () => {
    expect(css).not.toMatch(/#[0-9a-f]{3,8}\b/i);
    expect(css).not.toMatch(/\b(?:rgb|rgba|hsl|hsla|oklch|color-mix)\(/i);
    expect(css).not.toMatch(/:\s*(?:white|black)\b/i);
  });

  it('sets the redirect URI and issuers in the mono face and breaks them anywhere', () => {
    for (const selector of ['.sso-redirect-uri', '.sso-provider-issuer']) {
      expect(rule(css, selector), selector).toContain('font-family: var(--ui-font-mono)');
      expect(rule(css, selector), selector).toContain('overflow-wrap: anywhere');
    }
  });

  it('lets every column shrink below its content', () => {
    for (const selector of [
      '.sso-section',
      '.sso-card-header',
      '.sso-redirect, .sso-providers',
      '.sso-redirect-row',
      '.sso-redirect-uri',
      '.sso-provider',
      '.sso-provider-identity',
      '.sso-provider-heading',
      '.sso-provider-fields',
      '.sso-switch',
      '.sso-switch-text',
    ]) {
      expect(rule(css, selector), selector).toContain('min-width: 0');
    }
    expect(rule(css, '.sso-provider')).toContain('grid-template-columns: minmax(0, 1fr) auto');
  });

  it('colors the switch from the accent only when it is on', () => {
    expect(rule(css, '.sso-switch-control')).toContain('background-color: var(--ui-bg-active)');
    expect(rule(css, ".sso-switch-control[aria-checked='true']")).toContain(
      'background-color: var(--ui-accent)'
    );
    expect(rule(css, ".sso-switch-control[aria-checked='true'] .sso-switch-thumb")).toContain(
      'transform: translateX(0.875rem)'
    );
    expect(rule(css, '.sso-switch-control:focus-visible')).toContain(
      'outline: 2px solid var(--ui-accent)'
    );
    expect(rule(css, '.sso-switch-control:disabled')).toContain('cursor: not-allowed');
  });

  it('stacks provider actions under each provider and wraps headers on a phone', () => {
    const phone = media(css, PHONE);
    expect(rule(phone, '.sso-provider')).toContain('grid-template-columns: minmax(0, 1fr)');
    expect(rule(phone, '.sso-card-header, .sso-redirect-row')).toContain('flex-wrap: wrap');
  });
});
