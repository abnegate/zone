import { describe, expect, it } from 'bun:test';
import { existsSync, readFileSync } from 'node:fs';
import { dirname, join, relative } from 'node:path';

const IMPORT = /^\s*(?:import|export)\s[^'"]*?from\s+'([^']+)'/gm;

const SOURCE_ROOT = join(import.meta.dir, '../../..');

const UI_PACKAGE = '@zone/ui';

const COMPONENTS = '/components/';

const EXTENSIONS = ['.ts', '.tsx', '/index.ts', '/index.tsx'];

const E2E_SPEC = join(import.meta.dir, '../../../../e2e/audit-export.e2e.ts');

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

const E2E_MODULES = specifiers(E2E_SPEC)
  .filter((specifier) => specifier.startsWith('.'))
  .map((specifier) => resolved(E2E_SPEC, specifier))
  .filter((path) => path.startsWith(`${import.meta.dir}/`));

describe('the modules e2e specs import', () => {
  it('labels.ts imports nothing', () => {
    expect(specifiers(join(import.meta.dir, 'labels.ts'))).toEqual([]);
  });

  it('finds every audit module the e2e spec imports', () => {
    expect(E2E_MODULES.map((path) => relative(import.meta.dir, path)).sort()).toEqual([
      'filename.ts',
      'fixtures.ts',
      'labels.ts',
      'messages.ts',
      'schemas.ts',
      'scope.ts',
      'types.ts',
    ]);
  });

  for (const module of E2E_MODULES) {
    it(`${relative(import.meta.dir, module)} never reaches @zone/ui or a component`, () => {
      const { modules, packages } = reachable(module);

      expect([...packages].filter((name) => name.startsWith(UI_PACKAGE))).toEqual([]);
      expect(
        [...modules]
          .map((path) => `/${relative(SOURCE_ROOT, path)}`)
          .filter((path) => path.includes(COMPONENTS) || path.endsWith('.tsx'))
      ).toEqual([]);
    });
  }
});
