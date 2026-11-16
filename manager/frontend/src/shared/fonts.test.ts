import { describe, expect, it } from 'bun:test';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { FONT_MAP } from './context/ThemeContext';

const EXTERNAL = /https?:\/\/(?!www\.w3\.org)/;

const fonts = readFileSync(join(import.meta.dir, 'fonts.css'), 'utf8');
const imports = [...fonts.matchAll(/@import '([^']+)';/g)].map(([, specifier]) => specifier);

function declared(): string {
  return imports
    .map((specifier) => readFileSync(Bun.resolveSync(specifier, import.meta.dir), 'utf8'))
    .join('\n');
}

describe('self-hosted fonts', () => {
  it('imports only local packages', () => {
    expect(imports.length).toBeGreaterThan(0);
    expect(fonts).not.toMatch(EXTERNAL);
    for (const specifier of imports) {
      expect(specifier.startsWith('@fontsource/'), specifier).toBe(true);
    }
  });

  it('serves every font file from the bundle', () => {
    const faces = declared();
    expect(faces).not.toMatch(EXTERNAL);
    for (const specifier of imports) {
      const stylesheet = Bun.resolveSync(specifier, import.meta.dir);
      const css = readFileSync(stylesheet, 'utf8');
      for (const [, file] of css.matchAll(/url\(([^)]+)\)/g)) {
        expect(Bun.file(join(dirname(stylesheet), file)).size, file).toBeGreaterThan(0);
      }
    }
  });

  it('declares every family a workspace can choose', () => {
    const faces = declared();
    for (const stack of Object.values(FONT_MAP)) {
      const family = stack.match(/^"([^"]+)"/)?.[1];
      if (family) {
        expect(faces, family).toContain(`font-family: '${family}'`);
      }
    }
    expect(faces).toContain("font-family: 'IBM Plex Mono'");
  });

  it('keeps the page shell free of third-party hosts', () => {
    const shell = readFileSync(join(import.meta.dir, '../../index.html'), 'utf8');
    expect(shell).not.toMatch(EXTERNAL);
  });
});
