const DEFAULT_INSTANCE = 'https://gitlab.com';
const TRAILING_SLASHES = /\/+$/;
const API_BASE = /\/api\/v4$/;
const NUMERIC_PROJECT = /^\d+$/;
const PROJECTS_PATH = 'projects';
const WEB_PROTOCOLS = new Set(['http:', 'https:']);

export function gitLabProjectUrl(
  instance: string | undefined,
  project: string
): string | undefined {
  const named = project.trim();
  if (!named) return undefined;
  let url: URL;
  try {
    url = new URL(instance?.trim() || DEFAULT_INSTANCE);
  } catch {
    return undefined;
  }
  if (!WEB_PROTOCOLS.has(url.protocol)) return undefined;
  const path = url.pathname.replace(TRAILING_SLASHES, '').replace(API_BASE, '');
  const page = NUMERIC_PROJECT.test(named) ? `${PROJECTS_PATH}/${named}` : named;
  return `${url.origin}${path}/${page}`;
}
