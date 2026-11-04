const RELOADED_AT_KEY = 'zone.staleChunkReloadedAt';
const RELOAD_COOLDOWN_MILLISECONDS = 60_000;

type Reloadable = Pick<Window, 'addEventListener'> & {
  location: Pick<Location, 'reload'>;
};

export function reloadOnStaleChunk(
  target: Reloadable = window,
  storage: Pick<Storage, 'getItem' | 'setItem'> = sessionStorage,
  now: () => number = Date.now
): void {
  target.addEventListener('vite:preloadError', () => {
    const reloadedAt = Number(storage.getItem(RELOADED_AT_KEY) ?? 0);
    if (now() - reloadedAt < RELOAD_COOLDOWN_MILLISECONDS) {
      return;
    }
    storage.setItem(RELOADED_AT_KEY, String(now()));
    target.location.reload();
  });
}
