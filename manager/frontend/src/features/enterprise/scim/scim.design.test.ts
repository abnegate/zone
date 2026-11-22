import { describe, expect, it } from 'bun:test';
import { join } from 'node:path';
import { media, read, rule } from '../../../test/css';

const css = read(join(import.meta.dir, 'components', 'ScimSection.css'));

const PHONE = '(max-width: 768px)';

describe('SCIM section surfaces', () => {
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

  it('lets every column shrink below its content', () => {
    for (const selector of [
      '.scim-section',
      '.scim-card',
      '.scim-card-header',
      '.scim-card-title',
      '.scim-copy',
      '.scim-copy-row',
      '.scim-copy-value',
      '.scim-list',
      '.scim-item',
      '.scim-failure',
      '.scim-item-identity',
      '.scim-item-heading',
      '.scim-item-name',
      '.scim-item-meta',
      '.scim-token-hint, .scim-external-id, .scim-request',
      '.scim-failure-detail',
      '.scim-mapping',
    ]) {
      expect(rule(css, selector), selector).toContain('min-width: 0');
    }
    expect(rule(css, '.scim-item')).toContain('grid-template-columns: minmax(0, 1fr) auto');
  });

  it('breaks long secrets, URLs, names and paths instead of scrolling sideways', () => {
    for (const selector of [
      '.scim-card-title',
      '.scim-copy-value',
      '.scim-item-name',
      '.scim-item-meta',
      '.scim-token-hint, .scim-external-id, .scim-request',
      '.scim-failure-detail',
    ]) {
      expect(rule(css, selector), selector).toContain('overflow-wrap: anywhere');
    }
    expect(rule(css, '.scim-item-heading')).toContain('flex-wrap: wrap');
  });

  it('sets secrets, hints and request paths in the monospace face', () => {
    expect(rule(css, '.scim-copy-value')).toContain('font-family: var(--ui-font-mono)');
    expect(rule(css, '.scim-token-hint, .scim-external-id, .scim-request')).toContain(
      'font-family: var(--ui-font-mono)'
    );
  });

  it('stacks actions and mappings under each row, and dialog actions, on a phone', () => {
    const phone = media(css, PHONE);
    expect(rule(phone, '.scim-item')).toContain('grid-template-columns: minmax(0, 1fr)');
    expect(rule(phone, '.scim-card-header, .scim-copy-row')).toContain('flex-wrap: wrap');
    expect(rule(phone, '.scim-mapping .ui-select-control')).toContain('width: 100%');
    expect(rule(phone, '.scim-dialog .modal-actions')).toContain('flex-direction: column-reverse');
  });
});
