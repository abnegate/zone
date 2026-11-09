const DEFAULT_INSTANCE = 'https://gitlab.com';
const TRAILING_SLASHES = /\/+$/;
const API_BASE = /\/api\/v4$/;

export function gitLabProjectUrl(
  instance: string | undefined,
  project: string
): string | undefined {
  let url: URL;
  try {
    url = new URL(instance?.trim() || DEFAULT_INSTANCE);
  } catch {
    return undefined;
  }
  const path = url.pathname.replace(TRAILING_SLASHES, '').replace(API_BASE, '');
  return `${url.origin}${path}/${project}`;
}
