import { describe, expect, it } from 'bun:test';
import { join } from 'node:path';
import { media, read, rule } from '../../../test/css';

const css = read(join(import.meta.dir, 'components', 'RolesSection.css'));

const PHONE = '(max-width: 768px)';

describe('roles section surfaces', () => {
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
      '.roles-section',
      '.roles-card',
      '.roles-card-header',
      '.roles-list',
      '.roles-item',
      '.roles-item-identity',
      '.roles-item-heading',
      '.roles-item-name',
      '.roles-item-description',
      '.roles-editor',
      '.roles-fields',
      '.roles-group',
      '.roles-scopes',
      '.roles-scope-option',
      '.roles-scope-copy, .roles-scope-fixed, .roles-description-fixed',
      '.roles-description-text',
      '.roles-description-input',
      '.roles-permissions',
      '.roles-permission',
    ]) {
      expect(rule(css, selector), selector).toContain('min-width: 0');
    }
    expect(rule(css, '.roles-item')).toContain('grid-template-columns: minmax(0, 1fr) auto');
  });

  it('breaks long names and descriptions instead of scrolling sideways', () => {
    for (const selector of [
      '.roles-item-name',
      '.roles-item-description',
      '.roles-description-text',
    ]) {
      expect(rule(css, selector), selector).toContain('overflow-wrap: anywhere');
    }
    expect(rule(css, '.roles-item-heading')).toContain('flex-wrap: wrap');
    expect(rule(css, '.roles-description-text')).toContain('white-space: pre-wrap');
  });

  it('flows permissions and scopes into as many columns as fit, down to one', () => {
    for (const selector of ['.roles-permissions', '.roles-scopes']) {
      expect(rule(css, selector), selector).toContain(
        'grid-template-columns: repeat(auto-fill, minmax(min(100%, 16rem), 1fr))'
      );
    }
  });

  it('greys out a scope that cannot be chosen', () => {
    expect(rule(css, ".roles-scope-option input[type='radio']")).toContain(
      'accent-color: var(--ui-accent)'
    );
    expect(
      rule(
        css,
        ".roles-scope-option:has(input:disabled), .roles-scope-option:has(input:disabled) input[type='radio']"
      )
    ).toContain('cursor: not-allowed');
  });

  it('stacks role actions under each role and the editor actions on a phone', () => {
    const phone = media(css, PHONE);
    expect(rule(phone, '.roles-item')).toContain('grid-template-columns: minmax(0, 1fr)');
    expect(rule(phone, '.roles-card-header')).toContain('flex-wrap: wrap');
    expect(rule(phone, '.roles-editor .modal-actions')).toContain('flex-direction: column-reverse');
  });
});
