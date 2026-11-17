import { cpSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { site, unconfirmed } from '../config.ts';
import { pages } from './pages.ts';
import { renderPage } from './render.ts';

export const websiteDirectory = join(import.meta.dir, '..');
export const outputDirectory = join(websiteDirectory, 'dist');

const templateDirectory = join(websiteDirectory, 'templates');
const staticDirectory = join(websiteDirectory, 'static');

export function renderSite(basePath: string): ReadonlyMap<string, string> {
  const layout = readFileSync(join(templateDirectory, 'layout.html'), 'utf8');
  return new Map(
    pages.map((page) => {
      const template = readFileSync(join(templateDirectory, page.template), 'utf8');
      return [page.output, renderPage(page, pages, layout, template, site, basePath)];
    })
  );
}

export function build(basePath: string): void {
  rmSync(outputDirectory, { recursive: true, force: true });
  for (const [output, html] of renderSite(basePath)) {
    const destination = join(outputDirectory, output);
    mkdirSync(dirname(destination), { recursive: true });
    writeFileSync(destination, html);
  }
  cpSync(staticDirectory, join(outputDirectory, 'static'), { recursive: true });
}

function warnUnconfirmed(): void {
  const prefix = process.env.GITHUB_ACTIONS === 'true' ? '::warning::' : 'warning: ';
  for (const key of unconfirmed) {
    console.warn(`${prefix}website/config.ts: '${key}' is not confirmed yet`);
  }
}

if (import.meta.main) {
  build(process.env.SITE_BASE_PATH || '/');
  warnUnconfirmed();
  console.log(`Built ${pages.length} pages into ${outputDirectory}`);
}
