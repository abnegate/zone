import { describe, expect, it } from 'bun:test';
import { existsSync, readdirSync, readFileSync } from 'node:fs';
import { dirname, join, relative, resolve } from 'node:path';

const source = resolve(import.meta.dir, '..');
const cascade = join(import.meta.dir, 'cascade.ts');
const stylesheetImport = /^import '([^']+\.css)';$/gm;

function imports(file: string): string[] {
  return [...readFileSync(file, 'utf8').matchAll(stylesheetImport)]
    .map((match) => match[1])
    .filter((target) => target.startsWith('.'))
    .map((target) => resolve(dirname(file), target));
}

function modules(directory: string): string[] {
  return readdirSync(directory, { withFileTypes: true }).flatMap((entry) => {
    const path = join(directory, entry.name);
    if (entry.isDirectory()) {
      return modules(path);
    }
    return /\.tsx?$/.test(entry.name) && !/\.test\.tsx?$/.test(entry.name) ? [path] : [];
  });
}

describe('stylesheet cascade', () => {
  const listed = imports(cascade);

  it('lists every stylesheet a module imports', () => {
    const missing = modules(source)
      .flatMap(imports)
      .filter((stylesheet) => !listed.includes(stylesheet))
      .map((stylesheet) => relative(source, stylesheet));
    expect(missing).toEqual([]);
  });

  it('lists only stylesheets that exist', () => {
    expect(listed.filter((stylesheet) => !existsSync(stylesheet))).toEqual([]);
  });

  const lines = readFileSync(cascade, 'utf8').trim().split('\n');

  it('starts with the signed out sheets so splitting them off keeps their order', () => {
    expect(lines.slice(0, 2)).toEqual([
      "import '../shared/components/ZoneLogo/ZoneLogo.css';",
      "import '../features/auth/pages/AuthPage.css';",
    ]);
  });

  it('ends with the shared sheets so they keep winning ties', () => {
    expect(lines.slice(-2)).toEqual([
      "import '../App.css';",
      "import '@zone/ui/styles/globals.css';",
    ]);
  });
});
