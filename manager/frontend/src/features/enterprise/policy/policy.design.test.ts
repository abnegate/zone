import { describe, expect, it } from 'bun:test';
import { join } from 'node:path';
import { media, read, rule } from '../../../test/css';

const css = read(join(import.meta.dir, 'components', 'PolicySection.css'));

const PHONE = '(max-width: 768px)';

describe('agent policy section surfaces', () => {
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
      '.policy-section',
      '.policy-form',
      '.policy-fields',
      '.policy-field',
      '.policy-field-header',
      '.policy-field-summary',
      '.policy-field-summary-value',
      '.policy-options',
      '.policy-option',
      '.policy-chips',
      '.policy-chip',
      '.policy-chip-text',
      '.policy-entry',
      '.policy-ceiling',
      '.policy-ceiling-option',
      '.policy-ceiling-copy',
    ]) {
      expect(rule(css, selector), selector).toContain('min-width: 0');
    }
  });

  it('wraps long entries instead of scrolling sideways', () => {
    expect(rule(css, '.policy-chips')).toContain('flex-wrap: wrap');
    expect(rule(css, '.policy-chip')).toContain('max-width: 100%');
    expect(rule(css, '.policy-chip-text')).toContain('overflow-wrap: anywhere');
    expect(rule(css, '.policy-field-summary-value')).toContain('overflow-wrap: anywhere');
    expect(rule(css, '.policy-options')).toContain(
      'grid-template-columns: repeat(auto-fill, minmax(min(100%, 13rem), 1fr))'
    );
  });

  it('sets entries in the mono face', () => {
    expect(rule(css, '.policy-chip')).toContain('font-family: var(--ui-font-mono)');
  });

  it('colors the workspace badge from the accent and keeps the others quiet', () => {
    expect(rule(css, '.policy-badge--workspace')).toContain('color: var(--ui-accent)');
    expect(rule(css, '.policy-badge--organization')).toContain('color: var(--ui-text-secondary)');
    expect(rule(css, '.policy-badge--none')).toContain('color: var(--ui-text-muted)');
  });

  it('shows options the organization denies as muted and not clickable', () => {
    expect(rule(css, '.policy-option--denied .ui-label')).toContain('cursor: not-allowed');
    expect(rule(css, '.policy-ceiling-option:has(input:disabled) .policy-ceiling-label')).toContain(
      'color: var(--ui-text-muted)'
    );
  });

  it('keeps field controls at the end of a wrapping header', () => {
    expect(rule(css, '.policy-field-header')).toContain('flex-wrap: wrap');
    expect(rule(css, '.policy-field-control')).toContain('margin-inline-start: auto');
  });

  it('stacks summaries and actions on a phone', () => {
    const phone = media(css, PHONE);
    expect(rule(phone, '.policy-field-summary')).toContain('grid-template-columns: minmax(0, 1fr)');
    expect(rule(phone, '.policy-actions')).toContain('flex-direction: column-reverse');
  });
});
