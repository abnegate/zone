export const LAST_PATH_KEY = 'manager_last_path';

const AUTH_PATHS = new Set([
  '/login',
  '/register',
  '/verify-email',
  '/verify',
  '/forgot-password',
  '/reset-password',
  '/invitations',
  '/unauthorized',
  '/agent-sign-in',
]);

export type PathLike = {
  pathname: string;
  search?: string;
  hash?: string;
};

let bootRestorePending = false;

function hrefOf(path: PathLike): string {
  return `${path.pathname}${path.search ?? ''}${path.hash ?? ''}`;
}

function pathnameOf(href: string): string {
  return href.split(/[?#]/, 1)[0] || '/';
}

function isBareRoot(href: string): boolean {
  return href === '/' || href === '';
}

export function lastPathToRestore(current: string, storage: Storage = localStorage): string | null {
  if (!isBareRoot(current)) {
    return null;
  }
  const saved = storage.getItem(LAST_PATH_KEY);
  if (!saved || saved === current || AUTH_PATHS.has(pathnameOf(saved))) {
    return null;
  }
  return saved;
}

export function persistLastPath(href: string, storage: Storage = localStorage): void {
  if (AUTH_PATHS.has(pathnameOf(href))) {
    return;
  }
  if (bootRestorePending && isBareRoot(href)) {
    return;
  }
  bootRestorePending = false;
  storage.setItem(LAST_PATH_KEY, href);
}

export function persistCurrentPath(
  location: Pick<Location, 'pathname' | 'search' | 'hash'> = window.location,
  storage: Storage = localStorage
): void {
  persistLastPath(`${location.pathname}${location.search}${location.hash}`, storage);
}

export function restoreLastPath(
  current: string = `${window.location.pathname}${window.location.search}${window.location.hash}`,
  history: Pick<History, 'replaceState' | 'state'> = window.history,
  storage: Storage = localStorage
): string {
  const saved = lastPathToRestore(current, storage);
  if (!saved) {
    return current;
  }
  history.replaceState(history.state, '', saved);
  bootRestorePending = true;
  return saved;
}

export function pathAfterAuth(from?: PathLike | null, storage: Storage = localStorage): string {
  if (from && !AUTH_PATHS.has(from.pathname)) {
    return hrefOf(from);
  }
  const saved = storage.getItem(LAST_PATH_KEY);
  if (saved && !AUTH_PATHS.has(pathnameOf(saved))) {
    return saved;
  }
  return '/';
}
