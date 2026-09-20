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

  it('leaves every modal to the kit dialog', () => {
    const modals = read(join(styles, 'modals.css'));
    expect(modals).not.toContain('::before');
    expect(modals).not.toContain('.modal-content');
    expect(modals).not.toContain('.modal-close');
    expect(modals).not.toContain('.modal-details');
    const invitations = read(
      join(
        styles,
        '..',
        'features',
        'settings',
        'organization',
        'components',
        'InvitationsSection.css'
      )
    );
    expect(invitations).not.toContain('.invitation-dialog');
    expect(invitations).not.toContain('.modal-close');
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

  it('stacks a toggle description under its title in a 44px row: 1 + 4 + 18 + 16 + 4 + 1', () => {
    const forms = read(join(styles, 'forms.css'));
    const label = rule(forms, '  .toggle-label');
    expect(label).toContain('padding: var(--ui-space-1) var(--ui-space-3)');
    expect(label).toContain('margin: 0');
    expect(rule(forms, '  .toggle-wrapper')).not.toContain('margin-top');
    expect(rule(forms, '  .toggle-text')).toContain('flex-direction: column');
    expect(rule(forms, '  .toggle-text')).not.toContain('gap');
    expect(rule(forms, '  .toggle-title')).toContain('line-height: 1.125rem');
    expect(rule(forms, '  .toggle-desc')).toContain('line-height: var(--ui-space-4)');
    expect(rule(forms, '  .toggle-desc')).not.toContain('white-space');
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

  it('outlines every destructive button and only tints it on hover', () => {
    const globals = read(join(kit, 'globals.css'));
    const destructive = rule(globals, '  .ui-btn-destructive');
    expect(destructive).toContain('border-color: var(--ui-error)');
    expect(destructive).toContain('background-color: transparent');
    expect(destructive).toContain('color: var(--ui-error-600)');
    const hover = rule(globals, '  .ui-btn-destructive:hover');
    expect(hover).toContain('color-mix(');
    expect(hover).not.toContain('var(--ui-text-inverse)');
    expect(globals).not.toContain('.ui-btn-destructive-outline');
    expect(read(join(styles, 'buttons.css'))).not.toContain('.btn-danger {');
  });

  it('gives a dialog title a 24px line so the panel lands on the grid', () => {
    const globals = read(join(kit, 'globals.css'));
    expect(rule(globals, '  .ui-dialog-title')).toContain('line-height: var(--ui-space-6)');
  });

  it('seats a dialog badge beside the title on one row', () => {
    const globals = read(join(kit, 'globals.css'));
    expect(rule(globals, '  .ui-dialog-title-row')).toContain('display: flex');
    expect(rule(globals, '  .ui-dialog-title-row')).toContain('align-items: center');
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

describe('page chrome', () => {
  const app = join(import.meta.dir, '..');

  it('keeps the context switcher at control height with a 12 + 16 label stack', () => {
    const css = read(join(app, 'shared', 'components', 'ContextSwitcher', 'ContextSwitcher.css'));
    expect(rule(css, '.context-switcher-button')).toContain('height: var(--ui-control-height)');
    expect(rule(css, '.org-name')).toContain('line-height: 0.75rem');
    expect(rule(css, '.ws-name')).toContain('line-height: 1rem');
    expect(rule(css, '.context-label')).not.toContain('gap');
  });

  it('replaces the Train file pickers with 96px dashed drop zones and right-aligns Train', () => {
    const zone = read(join(app, 'features', 'models', 'components', 'DropZone.css'));
    expect(rule(zone, '.drop-zone')).toContain('min-height: 6rem');
    expect(rule(zone, '.drop-zone')).toContain('1px dashed');
    expect(rule(zone, '.drop-zone svg')).toContain('width: 1.25rem');
    expect(rule(zone, '.drop-zone-prompt')).toContain('var(--ui-text-sm)');
    expect(rule(zone, '.drop-zone-hint')).toContain('var(--ui-text-2xs)');

    const train = read(join(app, 'features', 'models', 'components', 'TrainPanel.css'));
    expect(rule(train, '.train-identity,\n.train-drops')).toContain(
      'grid-template-columns: repeat(2, minmax(0, 1fr))'
    );
    expect(rule(train, '.train-identity,\n.train-drops')).toContain(
      'gap: var(--ui-space-3) var(--ui-space-4)'
    );
    expect(rule(train, '.train-footer')).toContain('justify-content: flex-end');
    expect(rule(train, '.train-caption')).toContain('display: flex');
  });

  it('keeps a training target row at 72px including its border', () => {
    const train = read(join(app, 'features', 'models', 'components', 'TrainPanel.css'));
    const pair = rule(train, '.train-pair');
    expect(pair).toContain('box-sizing: border-box');
    expect(pair).toContain('min-height: 4.5rem');
    expect(pair).toContain('border: 1px solid var(--ui-border)');
    expect(rule(train, '.train-pair-fields .ui-input')).toContain(
      'height: var(--ui-control-height-sm)'
    );
  });
});
