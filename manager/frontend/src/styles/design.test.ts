import { describe, expect, it } from 'bun:test';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';

const styles = join(import.meta.dir);
const kit = join(import.meta.dir, '..', '..', '..', '..', 'packages', 'ui', 'src', 'styles');

function read(path: string): string {
  return readFileSync(path, 'utf8');
}

function token(css: string, name: string): string {
  const match = css.match(new RegExp(`--${name}:\\s*([^;]+);`));
  if (!match) throw new Error(`token --${name} is not defined`);
  return match[1].trim();
}

function rule(css: string, selector: string): string {
  const escaped = selector.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
  const match = css.match(new RegExp(`(^|\\n)${escaped}\\s*\\{([^}]*)\\}`));
  if (!match) throw new Error(`rule ${selector} is not defined`);
  return match[2];
}

describe('design tokens', () => {
  const variables = read(join(kit, 'variables.css'));

  it('sizes controls at 32 / 28 / 36', () => {
    expect(token(variables, 'ui-control-height')).toBe('2rem');
    expect(token(variables, 'ui-control-height-sm')).toBe('1.75rem');
    expect(token(variables, 'ui-control-height-lg')).toBe('2.25rem');
  });

  it('keeps the page bar at 48 and the gutters at 20 / 16', () => {
    expect(token(variables, 'ui-header-height')).toBe('3rem');
    expect(token(variables, 'ui-gutter')).toBe('1.25rem');
    expect(token(variables, 'ui-panel-padding')).toBe('1rem');
  });

  it('caps titles at 18 and headings at 16', () => {
    expect(token(variables, 'ui-title-size')).toBe('1.125rem');
    expect(token(variables, 'ui-heading-size')).toBe('1rem');
  });

  it('defines the frame, badge and row tokens the pages build on', () => {
    expect(token(variables, 'ui-sidebar-width')).toBe('14rem');
    expect(token(variables, 'ui-sidebar-collapsed')).toBe('3.5rem');
    expect(token(variables, 'ui-badge-height')).toBe('1.25rem');
    expect(token(variables, 'ui-list-row')).toBe('2.5rem');
    expect(token(variables, 'ui-list-row-2')).toBe('3.5rem');
    expect(token(variables, 'ui-content-width')).toBe('60rem');
  });
});

describe('shared surfaces', () => {
  it('lets a card overflow so a page can scroll it', () => {
    const cards = read(join(styles, 'cards.css'));
    expect(rule(cards, '.card')).not.toContain('overflow');
    expect(rule(cards, '.card')).not.toContain('margin-bottom');
    expect(cards).not.toContain('::before');
    expect(cards).not.toContain('translateY');
  });

  it('draws no gradient band over a modal', () => {
    const modals = read(join(styles, 'modals.css'));
    expect(modals).not.toContain('::before');
    expect(modals).toContain('.modal-content--sm');
    expect(modals).toContain('.modal-content--lg');
  });

  it('gives a primary link button the same ink as a primary button', () => {
    const buttons = read(join(styles, 'buttons.css'));
    expect(buttons).toMatch(/a\.btn\.btn-primary[^{]*\{[^}]*color: var\(--ui-text-inverse\)/);
  });

  it('bounds workspace pages to the viewport with one scrolling body', () => {
    const layout = read(join(styles, '..', 'shared', 'components', 'Layout', 'Layout.css'));
    expect(rule(layout, '.page--workspace')).toContain('height: 100%');
    expect(rule(layout, '.page--workspace:has(> .page-bar)')).toContain('flex-direction: column');
    expect(rule(layout, '.page-body')).toContain('overflow-y: auto');
    expect(rule(layout, '.page-body')).toContain('min-height: 0');
    expect(rule(layout, '.page-bar')).toContain('height: var(--ui-header-height)');
  });

  it('never lets the document itself scroll behind a workspace page', () => {
    const layout = read(join(styles, '..', 'shared', 'components', 'Layout', 'Layout.css'));
    expect(rule(layout, 'html:has(.page--workspace),\nbody:has(.page--workspace)')).toContain(
      'overflow: hidden'
    );
  });

  it('hides screen-reader text without laying it out against the page', () => {
    const utilities = read(join(styles, 'utilities.css'));
    const hidden = rule(utilities, '.sr-only');
    expect(hidden).toContain('position: absolute');
    expect(hidden).toContain('clip-path: inset(50%)');
    expect(hidden).toContain('width: 1px');
    expect(hidden).toContain('height: 1px');
    expect(hidden).toContain('overflow: hidden');
  });

  it('outlines a destructive button and fills it only on hover', () => {
    const globals = read(join(kit, 'globals.css'));
    const outline = rule(globals, '  .ui-btn-destructive-outline');
    expect(outline).toContain('background-color: transparent');
    expect(outline).toContain('color: var(--ui-error-600)');
    expect(rule(globals, '  .ui-btn-destructive-outline:hover')).toContain(
      'background-color: var(--ui-error-600)'
    );
  });

  it('gives a dialog title a 24px line so the panel lands on the grid', () => {
    const globals = read(join(kit, 'globals.css'));
    expect(rule(globals, '  .ui-dialog-title')).toContain('line-height: var(--ui-space-6)');
  });

  it('tints badges instead of filling them', () => {
    const globals = read(join(kit, 'globals.css'));
    expect(rule(globals, '  .ui-badge')).toContain('height: var(--ui-badge-height)');
    for (const variant of ['success', 'warning', 'info', 'destructive']) {
      expect(rule(globals, `  .ui-badge-${variant}`)).toContain('color-mix(');
    }
    expect(globals).toContain('.ui-badge-neutral');
    expect(globals).toContain('.ui-badge-accent');
  });
});
