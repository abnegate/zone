/**
 * Remembered list settings, read and written where storage may refuse.
 *
 * A browser with storage blocked throws from `localStorage`, and these are
 * read in `useState` initialisers: an unguarded throw there takes the whole
 * chats page down rather than one remembered choice. A setting that cannot be
 * stored applies for the session instead, which is the worst it should cost.
 */
export function readStored(key: string): string | null {
  try {
    return localStorage.getItem(key);
  } catch {
    return null;
  }
}

export function writeStored(key: string, value: string): void {
  try {
    localStorage.setItem(key, value);
  } catch {
    // Storage is unavailable, so the choice applies for this session only.
  }
}

/** The stored value when it is one of `allowed`, and `fallback` otherwise. */
export function readAllowed<T extends string>(key: string, allowed: readonly T[], fallback: T): T {
  const value = readStored(key);
  return value !== null && (allowed as readonly string[]).includes(value) ? (value as T) : fallback;
}
