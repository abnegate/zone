import { describe, expect, it } from 'bun:test';
import { existsSync, readFileSync } from 'node:fs';
import { join } from 'node:path';

const settings = join(import.meta.dir);
const sources = join(import.meta.dir, '..', 'sources');
const pages = join(import.meta.dir, '..', '..', 'pages');
const styles = join(import.meta.dir, '..', '..', 'styles');

function read(path: string): string {
  return readFileSync(path, 'utf8');
}

function rule(css: string, selector: string): string {
  const escaped = selector.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
  const match = css.match(new RegExp(`(^|\\n)${escaped}\\s*\\{([^}]*)\\}`));
  if (!match) throw new Error(`rule ${selector} is not defined`);
  return match[2];
}

describe('settings surfaces', () => {
  const shell = read(join(settings, 'workspace', 'pages', 'WorkspaceSettingsPage.css'));

  it('paints a flat page and a body that centres at the content width', () => {
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

  it('keeps the save row in view as a 48px sticky footer', () => {
    const actions = rule(shell, '.settings-actions');
    expect(actions).toContain('position: sticky');
    expect(actions).toContain('min-height: var(--ui-toolbar-height)');
  });

  it('draws the theme radius picker as a 32px segmented control', () => {
    expect(rule(shell, '.radio-group')).toContain('height: var(--ui-control-height)');
    expect(rule(shell, '.radio-option')).toContain('height: var(--ui-control-height-sm)');
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

  it('keeps metric values at 18 or below', () => {
    const billing = read(join(settings, 'organization', 'components', 'BillingSection.css'));
    expect(rule(billing, '.metric-value')).toContain('font-size: var(--ui-text-lg)');
    expect(billing).not.toContain('--ui-text-3xl');
    expect(billing).not.toContain('--ui-text-2xl');
  });
});

describe('sources surfaces', () => {
  const css = read(join(sources, 'pages', 'SourcesPage.css'));

  it('paints a flat page whose cards are list cards', () => {
    expect(css).not.toContain('radial-gradient');
    expect(css).not.toContain('.sources-header');
    expect(rule(css, '.sources-list')).toContain('gap: var(--ui-space-3)');
    expect(rule(css, '.source-card.card--list')).toContain(
      'padding: var(--ui-space-2) var(--ui-space-3)'
    );
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
    expect(css).not.toContain('line-clamp');
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
