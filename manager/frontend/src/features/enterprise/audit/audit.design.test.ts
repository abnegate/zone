import { describe, expect, it } from 'bun:test';
import { join } from 'node:path';
import { media, read, rule } from '../../../test/css';

const css = read(join(import.meta.dir, 'components', 'AuditExportSection.css'));

const PHONE = '(max-width: 768px)';

describe('audit export section surfaces', () => {
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
      '.audit-section',
      '.audit-card',
      '.audit-card-header',
      '.audit-card-copy',
      '.audit-destination-list, .audit-holder-list',
      '.audit-destination',
      '.audit-destination-identity',
      '.audit-destination-heading',
      '.audit-destination-name',
      '.audit-destination-target',
      '.audit-stats',
      '.audit-stat',
      '.audit-destination-controls',
      '.audit-destination-actions',
      '.audit-holders',
      '.audit-holder',
      '.audit-holder-name',
      '.audit-retention-fields',
      '.audit-export-fields',
      '.audit-actions',
      '.audit-fields',
      '.audit-field-row',
      '.audit-secrets',
      '.audit-secret',
      '.audit-secret-header',
      '.audit-certificate',
    ]) {
      expect(rule(css, selector), selector).toContain('min-width: 0');
    }
  });

  it('wraps long names, targets and errors instead of scrolling sideways', () => {
    for (const selector of [
      '.audit-destination-name',
      '.audit-destination-target',
      '.audit-stat dd',
      '.audit-holder-name',
      '.audit-holder-wait',
      '.audit-saved',
      '.audit-stalled, .audit-warning',
    ]) {
      expect(rule(css, selector), selector).toContain('overflow-wrap: anywhere');
    }
    expect(rule(css, '.audit-destination-heading')).toContain('flex-wrap: wrap');
    expect(rule(css, '.audit-stats')).toContain('flex-wrap: wrap');
    expect(rule(css, '.audit-destination-actions')).toContain('flex-wrap: wrap');
    expect(rule(css, '.audit-certificate')).toContain('word-break: break-all');
    expect(rule(css, '.audit-export-fields')).toContain(
      'grid-template-columns: repeat(auto-fit, minmax(min(100%, 10rem), 1fr))'
    );
  });

  it('sets targets, URLs and certificates in the mono face', () => {
    expect(rule(css, '.audit-destination-target')).toContain('font-family: var(--ui-font-mono)');
    expect(rule(css, '.audit-certificate')).toContain('font-family: var(--ui-font-mono)');
    expect(rule(css, '.audit-mono')).toContain('font-family: var(--ui-font-mono)');
  });

  it('marks delivery errors and held destinations with the status colors', () => {
    expect(rule(css, '.audit-destination-error, .audit-row-error, .audit-test-failed')).toContain(
      'color: var(--ui-error)'
    );
    expect(rule(css, '.audit-test-result')).toContain('color: var(--ui-success)');
    expect(rule(css, '.audit-holder')).toContain(
      'border-inline-start: 2px solid var(--ui-warning)'
    );
  });

  it('styles the unencrypted warning itself, since the form renders outside the settings page', () => {
    const warning = rule(css, '.audit-warning');
    expect(warning).toContain('border-inline-start: 2px solid var(--ui-warning)');
    expect(warning).toContain('background-color: var(--ui-warning-muted)');
    expect(warning).toContain('padding: var(--ui-space-2) var(--ui-space-3)');
  });

  it('lines the ghost row actions up with the edge of the enabled switch above them', () => {
    expect(rule(css, '.audit-destination-actions')).toContain(
      'margin-inline-end: calc(-1 * var(--ui-space-2-5))'
    );
  });

  it('keeps the retention period field narrow above its keep-forever checkbox', () => {
    expect(rule(css, '.audit-retention-fields')).toContain('flex-direction: column');
    expect(rule(css, '.audit-retention-fields .ui-input-wrapper')).toContain(
      'width: min(100%, 16rem)'
    );
  });

  it('stacks rows, fields and actions on a phone', () => {
    const phone = media(css, PHONE);
    expect(rule(phone, '.audit-destination')).toContain('grid-template-columns: minmax(0, 1fr)');
    expect(rule(phone, '.audit-field-row')).toContain('grid-template-columns: minmax(0, 1fr)');
    expect(rule(phone, '.audit-card-header')).toContain('flex-wrap: wrap');
    expect(rule(phone, '.audit-actions')).toContain('flex-direction: column-reverse');
    expect(rule(phone, '.audit-destination-controls')).toContain('flex-direction: row');
  });
});
