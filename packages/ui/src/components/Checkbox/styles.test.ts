import { describe, expect, it } from 'bun:test';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';

const styles = join(import.meta.dir, '..', '..', 'styles');
const variables = readFileSync(join(styles, 'variables.css'), 'utf8');
const globals = readFileSync(join(styles, 'globals.css'), 'utf8');

function block(css: string, selector: string): string {
  const start = css.indexOf(`${selector} {`);
  if (start === -1) throw new Error(`block ${selector} is not defined`);
  return css.slice(start, css.indexOf('}', start));
}

function token(css: string, name: string): string {
  const match = css.match(new RegExp(`--${name}:\\s*([^;]+);`));
  if (!match) throw new Error(`token --${name} is not defined`);
  return match[1].trim();
}

function luminance(hex: string): number {
  const [red, green, blue] = [1, 3, 5].map((offset) => {
    const channel = Number.parseInt(hex.slice(offset, offset + 2), 16) / 255;
    return channel <= 0.04045 ? channel / 12.92 : ((channel + 0.055) / 1.055) ** 2.4;
  });
  return 0.2126 * red + 0.7152 * green + 0.0722 * blue;
}

function contrast(first: string, second: string): number {
  const [lighter, darker] = [luminance(first), luminance(second)].sort((a, b) => b - a);
  return (lighter + 0.05) / (darker + 0.05);
}

const themes = {
  light: block(variables, ":root[data-theme='light']"),
  dark: block(variables, ":root[data-theme='dark']"),
};

describe('checkbox styles', () => {
  it('rounds the box with the extra-small radius so it reads as a checkbox, not a radio', () => {
    expect(token(variables, 'ui-radius-xs')).toBe('0.25rem');
    const checkbox = block(globals, '  .ui-checkbox');
    expect(checkbox).toContain('border-radius: var(--ui-radius-xs)');
    expect(checkbox).toContain('border: 1px solid var(--ui-border-control)');
    expect(checkbox).toContain('background-color: transparent');
  });

  it('fills indeterminate boxes like checked ones', () => {
    expect(globals).toMatch(
      /\.ui-checkbox\[data-state='checked'\],\s*\.ui-checkbox\[data-state='indeterminate'\]\s*\{/
    );
  });

  it('offsets the focus ring so a focused filled box still shows a separate ring', () => {
    const focus = block(globals, '  .ui-checkbox:focus-visible');
    expect(focus).toContain('outline: 2px solid var(--ui-border-focus)');
    expect(focus).toContain('outline-offset: 2px');
  });

  for (const [theme, surfaces] of [
    ['light', ['ui-bg-base', 'ui-bg-elevated', 'ui-bg-surface']],
    ['dark', ['ui-bg-base', 'ui-bg-elevated', 'ui-bg-surface', 'ui-bg-hover']],
  ] as const) {
    for (const surface of surfaces) {
      it(`keeps the ${theme} unchecked edge at 3:1 or more against --${surface}`, () => {
        const css = themes[theme];
        expect(
          contrast(token(css, 'ui-border-control'), token(css, surface))
        ).toBeGreaterThanOrEqual(3);
      });
    }
  }
});
