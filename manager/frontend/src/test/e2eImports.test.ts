import { describe, expect, it } from 'bun:test';
import { existsSync, readFileSync } from 'node:fs';
import { dirname, join, relative } from 'node:path';
import { Glob } from 'bun';

const ROOT = join(import.meta.dir, '../..');

const ENTRY_PATTERNS = ['e2e/**/*.ts', 'playwright*.config.ts'];

const STATEMENT = /^\s*(?:import|export)\s+(type\s+)?([^'";]*?)\s*from\s+'([^']+)'/gm;

const SIDE_EFFECT = /^\s*import\s+'([^']+)'/gm;

const NAMED_ONLY = /^\{([^}]*)\}$/;

const TYPE_SPECIFIER = /^type\s/;

const EXTENSIONS = ['', '.ts', '.tsx', '/index.ts', '/index.tsx'];

const BUILD_TIME_ENVIRONMENT = /import\.meta\.env/;

function typeOnly(clause: string): boolean {
  const named = clause.trim().match(NAMED_ONLY);
  if (named === null) return false;
  const specifiers = named[1]
    .split(',')
    .map((specifier) => specifier.trim())
    .filter((specifier) => specifier.length > 0);
  return specifiers.length > 0 && specifiers.every((specifier) => TYPE_SPECIFIER.test(specifier));
}

function runtimeSpecifiers(source: string): string[] {
  const statements = [...source.matchAll(STATEMENT)]
    .filter(([, typeKeyword, clause]) => typeKeyword === undefined && !typeOnly(clause))
    .map(([, , , specifier]) => specifier);
  const sideEffects = [...source.matchAll(SIDE_EFFECT)].map(([, specifier]) => specifier);
  return [...statements, ...sideEffects];
}

function resolved(from: string, specifier: string): string {
  const base = join(dirname(from), specifier);
  const path = EXTENSIONS.map((extension) => `${base}${extension}`).find(
    (candidate) => existsSync(candidate) && /\.tsx?$/.test(candidate)
  );
  if (path === undefined) throw new Error(`${specifier} does not resolve from ${from}`);
  return path;
}

function entries(): string[] {
  return ENTRY_PATTERNS.flatMap((pattern) => [
    ...new Glob(pattern).scanSync({ cwd: ROOT, absolute: true }),
  ]).sort();
}

function environmentChains(entryPoints: string[]): string[] {
  const parents = new Map<string, string | null>(entryPoints.map((entry) => [entry, null]));
  const pending = [...entryPoints];
  const offenders: string[] = [];
  while (pending.length > 0) {
    const path = pending.shift() as string;
    const source = readFileSync(path, 'utf8');
    if (BUILD_TIME_ENVIRONMENT.test(source)) offenders.push(path);
    for (const specifier of runtimeSpecifiers(source)) {
      if (!specifier.startsWith('.')) continue;
      const target = resolved(path, specifier);
      if (parents.has(target)) continue;
      parents.set(target, path);
      pending.push(target);
    }
  }
  return offenders.map((offender) => {
    const chain: string[] = [];
    for (let path: string | null = offender; path !== null; path = parents.get(path) ?? null) {
      chain.unshift(relative(ROOT, path));
    }
    return chain.join(' -> ');
  });
}

describe('runtimeSpecifiers', () => {
  it('keeps value, namespace, re-export and side-effect imports', () => {
    const source = [
      "import { a } from './a';",
      "import b from './b';",
      "import * as c from './c';",
      "export { d } from './d';",
      "export * from './e';",
      "import './f';",
      "import { type G, h } from './h';",
      "import i, { type I } from './i';",
      'import {',
      '  j,',
      '  type J,',
      "} from './j';",
    ].join('\n');

    expect(runtimeSpecifiers(source).sort()).toEqual([
      './a',
      './b',
      './c',
      './d',
      './e',
      './f',
      './h',
      './i',
      './j',
    ]);
  });

  it('skips type-only imports and re-exports', () => {
    const source = [
      "import type { A } from './a';",
      "export type { B } from './b';",
      "import { type C, type D } from './c';",
      'import {',
      '  type E,',
      '  type F,',
      "} from './e';",
    ].join('\n');

    expect(runtimeSpecifiers(source)).toEqual([]);
  });
});

describe('the modules Playwright loads in Node', () => {
  it('finds the e2e specs and Playwright configs', () => {
    const found = entries().map((path) => relative(ROOT, path));

    expect(found).toContain('playwright.config.ts');
    expect(found).toContain('e2e/license.e2e.ts');
  });

  it('never reach import.meta.env, which only Vite defines', () => {
    expect(environmentChains(entries())).toEqual([]);
  });
});
