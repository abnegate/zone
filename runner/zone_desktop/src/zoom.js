// Zoom the webview from the keyboard, and keep the level where a reload can
// find it again. The View menu owns `CmdOrCtrl` with the main-row `=`, `-` and
// `0`, so its accelerators consume those before the page ever sees them; what
// is left here is everything a menu accelerator cannot match, because it binds
// a physical key: the shifted `+` and `_`, the numpad, and the layouts that
// carry `+` or `-` on some other key entirely.
(() => {
  const STORAGE_KEY = 'zone.zoom';
  const DEFAULT_LEVEL = 1;
  const LEVELS = [0.5, 0.67, 0.75, 0.8, 0.9, 1, 1.1, 1.25, 1.5, 1.75, 2, 2.5, 3];

  const onMac = globalThis.__ZONE_ZOOM_MAC__ === true;

  let level = stored();

  function stored() {
    try {
      const saved = Number(window.localStorage.getItem(STORAGE_KEY));
      return LEVELS.includes(saved) ? saved : DEFAULT_LEVEL;
    } catch {
      return DEFAULT_LEVEL;
    }
  }

  function apply(next) {
    level = next;
    try {
      window.localStorage.setItem(STORAGE_KEY, String(level));
    } catch {
      // A webview with storage disabled still zooms, it just forgets.
    }
    window.__TAURI_INTERNALS__?.invoke('plugin:webview|set_webview_zoom', {
      value: level,
    });
  }

  function step(direction) {
    const index = LEVELS.indexOf(level);
    const next = Math.min(Math.max(index + direction, 0), LEVELS.length - 1);
    apply(LEVELS[next]);
  }

  // `event.code` is the physical key the menu matches on, so a combo is ours
  // only when the character is right and the physical key is not one the menu
  // has already claimed.
  function direction(event) {
    if (event.code === 'NumpadAdd') return 1;
    if (event.code === 'NumpadSubtract') return -1;
    if (event.key === '+' || (event.key === '=' && event.code !== 'Equal')) {
      return 1;
    }
    if (event.key === '_' || (event.key === '-' && event.code !== 'Minus')) {
      return -1;
    }
    return 0;
  }

  function isReset(event) {
    return (
      event.code === 'Numpad0' || (event.key === '0' && event.code !== 'Digit0')
    );
  }

  window.addEventListener('keydown', (event) => {
    if (onMac ? !event.metaKey : !event.ctrlKey) return;
    if (event.altKey) return;

    const towards = direction(event);
    if (towards !== 0) {
      event.preventDefault();
      step(towards);
      return;
    }
    if (isReset(event)) {
      event.preventDefault();
      apply(DEFAULT_LEVEL);
    }
  });

  // What the View menu items call, so both routes move the same number.
  window.__zoneZoom = {
    in: () => step(1),
    out: () => step(-1),
    reset: () => apply(DEFAULT_LEVEL),
    get level() {
      return level;
    },
  };

  // The native zoom level outlives a page load while this script does not, so
  // restore on every load rather than letting the two drift apart.
  apply(level);
})();
