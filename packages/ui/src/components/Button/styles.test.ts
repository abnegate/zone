import { describe, expect, it } from 'bun:test';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';

const globals = readFileSync(join(import.meta.dir, '..', '..', 'styles', 'globals.css'), 'utf8');

function block(selector: string): string {
  const start = globals.indexOf(`${selector} {`);
  if (start === -1) throw new Error(`block ${selector} is not defined`);
  return globals.slice(start, globals.indexOf('}', start));
}

describe('button styles', () => {
  it('keeps a link styled as a button free of the hover underline every other link gets', () => {
    expect(block('  .ui-btn')).toContain('text-decoration: none');
  });

  it('still underlines the link variant on hover', () => {
    expect(block('  .ui-btn-link:hover')).toContain('text-decoration: underline');
  });

  it('declares the buttons in the components layer so they outrank base link styles', () => {
    const layer = globals.lastIndexOf('@layer components {', globals.indexOf('  .ui-btn {'));
    expect(layer).toBeGreaterThan(-1);
    expect(globals.slice(layer, globals.indexOf('  .ui-btn {'))).not.toMatch(/^}/m);
  });
});
