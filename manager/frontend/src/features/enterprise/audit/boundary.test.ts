import { describe, expect, it } from 'bun:test';
import { existsSync, readFileSync } from 'node:fs';
import { dirname, join, relative } from 'node:path';

const E2E_MODULES = ['labels.ts', 'schemas.ts', 'fixtures.ts', 'messages.ts', 'types.ts'];

const IMPORT = /^\s*(?:import|export)\s[^'"]*?from\s+'([^']+)'/gm;

const SOURCE_ROOT = join(import.meta.dir, '../../..');

const UI_PACKAGE = '@zone/ui';

const COMPONENTS = '/components/';

const EXTENSIONS = ['.ts', '.tsx', '/index.ts', '/index.tsx'];

function specifiers(path: string): string[] {
  return [...readFileSync(path, 'utf8').matchAll(IMPORT)].map((match) => match[1]);
}

function resolved(from: string, specifier: string): string {
  const base = join(dirname(from), specifier);
  const path = EXTENSIONS.map((extension) => `${base}${extension}`).find(existsSync);
  if (path === undefined) throw new Error(`${specifier} does not resolve from ${from}`);
  return path;
}

function reachable(entry: string): { modules: Set<string>; packages: Set<string> } {
  const modules = new Set<string>();
  const packages = new Set<string>();
  const pending = [entry];
  while (pending.length > 0) {
    const path = pending.pop() as string;
    if (modules.has(path)) continue;
    modules.add(path);
    for (const specifier of specifiers(path)) {
      if (specifier.startsWith('.')) {
        pending.push(resolved(path, specifier));
      } else {
        packages.add(specifier);
      }
    }
  }
  return { modules, packages };
}

describe('the modules e2e specs import', () => {
  it('labels.ts imports nothing', () => {
    expect(specifiers(join(import.meta.dir, 'labels.ts'))).toEqual([]);
  });

  for (const module of E2E_MODULES) {
    it(`${module} never reaches @zone/ui or a component`, () => {
      const { modules, packages } = reachable(join(import.meta.dir, module));

      expect([...packages].filter((name) => name.startsWith(UI_PACKAGE))).toEqual([]);
      expect(
        [...modules]
          .map((path) => `/${relative(SOURCE_ROOT, path)}`)
          .filter((path) => path.includes(COMPONENTS) || path.endsWith('.tsx'))
      ).toEqual([]);
    });
  }
});
