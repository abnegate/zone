export const SSO_ATTEMPT_KEY = 'zone_sso_attempt';

export interface SsoAttempt {
  state: string;
  next: string;
}

const SPACE = 0x20;
const DELETE = 0x7f;

function hasControlCharacter(value: string): boolean {
  for (let index = 0; index < value.length; index++) {
    const code = value.charCodeAt(index);
    if (code < SPACE || code === DELETE) {
      return true;
    }
  }
  return false;
}

export function safePath(value: unknown): string {
  if (
    typeof value !== 'string' ||
    !value.startsWith('/') ||
    value.startsWith('//') ||
    value.startsWith('/\\') ||
    hasControlCharacter(value)
  ) {
    return '/';
  }
  return value;
}

export function saveAttempt(attempt: SsoAttempt, storage: Storage = sessionStorage): void {
  storage.setItem(
    SSO_ATTEMPT_KEY,
    JSON.stringify({ state: attempt.state, next: safePath(attempt.next) })
  );
}

function parsed(json: string | null): SsoAttempt | null {
  if (!json) {
    return null;
  }
  try {
    const value: unknown = JSON.parse(json);
    if (!value || typeof value !== 'object') {
      return null;
    }
    const { state, next } = value as Record<string, unknown>;
    if (typeof state !== 'string' || !state) {
      return null;
    }
    return { state, next: safePath(next) };
  } catch {
    return null;
  }
}

export function readAttempt(storage: Storage = sessionStorage): SsoAttempt | null {
  try {
    return parsed(storage.getItem(SSO_ATTEMPT_KEY));
  } catch {
    return null;
  }
}

export function clearAttempt(storage: Storage = sessionStorage): void {
  try {
    storage.removeItem(SSO_ATTEMPT_KEY);
  } catch {
    // A browser that refuses storage holds no attempt to clear.
  }
}
