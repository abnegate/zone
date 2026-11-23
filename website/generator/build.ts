import { cpSync, mkdirSync, readdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { basename, dirname, join } from 'node:path';
import { site, unconfirmed } from '../config.ts';
import { pages } from './pages.ts';
import { renderPage } from './render.ts';

export const websiteDirectory = join(import.meta.dir, '..');
export const outputDirectory = join(websiteDirectory, 'dist');

const templateDirectory = join(websiteDirectory, 'templates');
const fragmentDirectory = join(templateDirectory, 'fragments');
const staticDirectory = join(websiteDirectory, 'static');

export function fragmentName(file: string): string {
  return basename(file, '.html').replace(/-([a-z])/g, (_, letter: string) => letter.toUpperCase());
}

function readFragments(): Record<string, string> {
  return Object.fromEntries(
    readdirSync(fragmentDirectory)
      .filter((file) => file.endsWith('.html'))
      .map((file) => [fragmentName(file), readFileSync(join(fragmentDirectory, file), 'utf8')])
  );
}

/// The address the site is served from, ending in a slash so pages resolve beneath it.
export function baseURLFrom(address: string): URL {
  return new URL(address.endsWith('/') ? address : `${address}/`);
}

export function defaultBaseURL(): URL {
  return baseURLFrom(`https://${site.domain}/`);
}

export function renderSite(baseURL: URL): ReadonlyMap<string, string> {
  const layout = readFileSync(join(templateDirectory, 'layout.html'), 'utf8');
  const fragments = readFragments();
  return new Map(
    pages.map((page) => {
      const template = readFileSync(join(templateDirectory, page.template), 'utf8');
      return [page.output, renderPage(page, pages, layout, template, fragments, site, baseURL)];
    })
  );
}

export function build(baseURL: URL): void {
  rmSync(outputDirectory, { recursive: true, force: true });
  for (const [output, html] of renderSite(baseURL)) {
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
  const address = process.env.SITE_BASE_URL;
  build(address ? baseURLFrom(address) : defaultBaseURL());
  warnUnconfirmed();
  console.log(`Built ${pages.length} pages into ${outputDirectory}`);
}
