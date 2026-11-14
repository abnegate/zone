import { describe, expect, it } from 'bun:test';
import { existsSync } from 'node:fs';
import { join } from 'node:path';
import { media, read, rule } from '../../test/css';

const settings = join(import.meta.dir);
const sources = join(import.meta.dir, '..', 'sources');
const pages = join(import.meta.dir, '..', '..', 'pages');
const styles = join(import.meta.dir, '..', '..', 'styles');

describe('settings surfaces', () => {
  const shell = read(join(settings, 'pages', 'SettingsPage.css'));

  it('paints a flat page', () => {
    expect(shell).not.toContain('radial-gradient');
    expect(shell).not.toContain('.settings-page-header');
    expect(shell).not.toContain('.settings-page-body');
  });

  it('sets section and card titles in the body face at 14', () => {
    expect(rule(shell, '.settings-page .section-title')).toContain(
      'font-family: var(--ui-font-body)'
    );
    expect(rule(shell, '.settings-page .section-title')).toContain('font-size: var(--ui-text-md)');
    expect(rule(shell, '.settings-page .card-title')).toContain('font-family: var(--ui-font-body)');
    expect(rule(shell, '.settings-page .card-title')).toContain('font-size: var(--ui-text-md)');
    expect(shell).not.toContain('--ui-heading-size');
  });

  it('pads a settings card at the card token and keeps cards 16 apart', () => {
    expect(rule(shell, '.settings-card')).toContain('padding: var(--ui-card-padding)');
    expect(rule(shell, '.settings-form')).toContain('gap: var(--ui-space-4)');
    expect(shell).not.toContain('var(--ui-space-6);\n  margin-bottom');
  });

  it('lays fields out on a two-column grid with 12 x 16 gaps', () => {
    const grid = rule(shell, '.form-grid');
    expect(grid).toContain('grid-template-columns: repeat(2, minmax(0, 1fr))');
    expect(grid).toContain('gap: var(--ui-field-gap) var(--ui-group-gap)');
    expect(shell).toContain('.form-grid > .form-group:only-child');
  });

  it('stacks a section row on a phone without widening the body past the viewport', () => {
    const phone = media(shell, '(max-width: 768px)');
    expect(rule(phone, '.section-row-copy')).toContain('align-self: stretch');
    expect(rule(phone, '.settings-page .section-description')).toContain('max-width: 100%');
    expect(rule(shell, '.settings-page .section-description')).toContain('text-overflow: ellipsis');
  });

  it('wraps a section description on a phone instead of cutting it off', () => {
    const phone = media(shell, '(max-width: 768px)');
    expect(rule(phone, '.settings-page .section-description')).toContain('white-space: normal');
  });

  it('renders the read-only theme fieldset invisibly and lets it shrink below its content', () => {
    const fieldset = rule(shell, '.settings-fieldset');
    expect(fieldset).toContain('min-width: 0');
    expect(fieldset).toContain('margin: 0');
    expect(fieldset).toContain('padding: 0');
    expect(fieldset).toContain('border: 0');
    expect(fieldset).toContain('gap: var(--ui-space-4)');
  });

  it('fades the disabled font-size slider and radius pills with a not-allowed cursor', () => {
    const locked = rule(
      shell,
      '.settings-form .form-slider:disabled, .settings-form fieldset:disabled .radio-option'
    );
    expect(locked).toContain('opacity: 0.5');
    expect(locked).toContain('cursor: not-allowed');
  });

  it('starts the shell at the left edge of the page body like every other page', () => {
    const layout = rule(shell, '.settings-layout');
    expect(layout).not.toContain('margin');
    expect(layout).toContain(
      'max-width: calc(var(--settings-nav-width) + var(--ui-space-6) + var(--ui-content-width))'
    );
  });

  it('lays the shell out as a sticky nav column beside the content', () => {
    const layout = rule(shell, '.settings-layout');
    expect(layout).toContain('display: grid');
    expect(layout).toContain('grid-template-columns: var(--settings-nav-width) minmax(0, 1fr)');
    const nav = rule(shell, '.settings-shell .settings-nav');
    expect(nav).toContain('position: sticky');
    expect(nav).toContain('top: 0');
  });

  it('stacks the shell into one column and unpins the nav on a phone', () => {
    const phone = media(shell, '(max-width: 768px)');
    expect(rule(phone, '.settings-layout')).toContain('grid-template-columns: minmax(0, 1fr)');
    expect(rule(phone, '.settings-shell .settings-nav')).toContain('position: static');
  });

  it('draws an inherited field as one control-height row', () => {
    const inherited = read(join(settings, 'workspace', 'components', 'InheritedField.css'));
    expect(rule(inherited, '.inherited-field')).toContain(
      'border-bottom: 1px solid var(--ui-border-subtle)'
    );
    const row = rule(inherited, '.inherited-field-row');
    expect(row).toContain('display: grid');
    expect(row).toContain('min-height: var(--ui-control-height)');
  });

  it('keeps the save row in view as a 48px sticky footer', () => {
    const actions = rule(shell, '.settings-actions');
    expect(actions).toContain('position: sticky');
    expect(actions).toContain('min-height: var(--ui-toolbar-height)');
  });

  it('draws the theme radius picker as a 32px segmented control with the pills centred', () => {
    expect(rule(shell, '.radio-group')).toContain('height: var(--ui-control-height)');
    expect(rule(shell, '.radio-option')).toContain('height: var(--ui-control-height-sm)');
    expect(rule(shell, '.radio-option')).toContain('margin: 0;');
  });

  it('tints role, status and action badges from the semantic palette at 20px', () => {
    const members = read(join(settings, 'organization', 'components', 'OrgMembersSection.css'));
    const billing = read(join(settings, 'organization', 'components', 'BillingSection.css'));
    const audit = read(join(settings, 'organization', 'components', 'AuditLogsSection.css'));
    for (const css of [members, billing, audit]) {
      expect(css).not.toMatch(/#[0-9a-f]{3,6}\b/i);
      expect(css).not.toContain('rgba(');
      expect(css).not.toContain('text-transform: uppercase;\n  letter-spacing: 0.5px');
    }
    expect(rule(members, '.role-badge')).toContain('height: var(--ui-badge-height)');
    expect(rule(billing, '.billing-section .status-badge')).toContain(
      'height: var(--ui-badge-height)'
    );
    expect(rule(audit, '.action-badge')).toContain('height: var(--ui-badge-height)');
    expect(rule(members, '.member-avatar')).toContain('width: var(--ui-space-6)');
    expect(rule(members, '.role-select')).toContain('height: var(--ui-control-height-sm)');
  });

  it('sets connect URLs in the body face at the control height', () => {
    const css = read(join(settings, 'server', 'components', 'ConnectDevicesSection.css'));
    expect(css).not.toMatch(/#[0-9a-f]{3,6}\b/i);
    expect(css).not.toContain('rgba(');
    expect(rule(css, '.connect-url')).toContain('font-family: var(--ui-font-mono)');
    expect(rule(css, '.connect-url-row')).toContain('min-height: var(--ui-control-height)');
  });

  it('keeps metric values at 18 or below', () => {
    const billing = read(join(settings, 'organization', 'components', 'BillingSection.css'));
    expect(rule(billing, '.metric-value')).toContain('font-size: var(--ui-text-lg)');
    expect(billing).not.toContain('--ui-text-3xl');
    expect(billing).not.toContain('--ui-text-2xl');
  });
});

describe('sources surfaces', () => {
  const css = read(join(sources, 'components', 'SourcesSection.css'));

  it('paints a flat section whose cards are list cards', () => {
    expect(css).not.toContain('radial-gradient');
    expect(css).not.toContain('.sources-page');
    expect(css).not.toContain('.sources-header');
    expect(css).not.toContain('.sources-list');
    expect(css).not.toContain('card--list');
    expect(rule(css, '.sources-table td')).toContain('height: var(--ui-list-row-2)');
    expect(rule(css, '.source-name')).toContain('text-overflow: ellipsis');
    expect(rule(css, '.source-url')).toContain('font-family: var(--ui-font-mono)');
  });

  it('lays wizard type tiles out three across at exactly the two-line row height', () => {
    expect(rule(css, '.source-type-grid')).toContain('repeat(3, minmax(0, 1fr))');
    expect(rule(css, '.source-type-grid')).toContain('gap: var(--ui-space-2)');
    expect(rule(css, '.source-type-option')).toContain('height: var(--ui-list-row-2)');
    expect(rule(css, '.source-type-option')).not.toContain('min-height');
  });

  it('keeps a tile name and description each to one line', () => {
    const lines = rule(css, '.source-type-name,\n.source-type-desc');
    expect(lines).toContain('white-space: nowrap');
    expect(lines).toContain('text-overflow: ellipsis');
    expect([...css.matchAll(/line-clamp:\s*([^;]+);/g)].map(([, value]) => value)).toEqual([
      'unset',
    ]);
  });
});

describe('form rows', () => {
  it('spans a lone field across both columns', () => {
    const forms = read(join(styles, 'forms.css'));
    expect(forms).toMatch(/\.form-row > \.form-group:only-child \{\s*grid-column: 1 \/ -1;/);
  });
});

describe('unauthorized page', () => {
  it('has no card of its own and renders through the auth card', () => {
    expect(existsSync(join(pages, 'UnauthorizedPage.css'))).toBe(false);
    const page = read(join(pages, 'UnauthorizedPage.tsx'));
    expect(page).toContain('<AuthCard>');
    expect(page).toContain('<AuthStatus');
    expect(page).not.toContain('unauthorized-card');
  });
});
