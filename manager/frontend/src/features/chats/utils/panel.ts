const PANEL_PREFIX = 'zone:chat-panel';

export const PANEL_WIDTH = {
  fallback: 320,
  min: 240,
  max: 560,
  step: 16,
  largeStep: 64,
} as const;

export type PanelSetting = 'width' | 'collapsed' | 'changes' | 'gallery' | 'sources';

/** Where one viewer's panel setting is kept, so people sharing a browser keep their own. */
export function panelKey(userId: string | null | undefined, setting: PanelSetting): string {
  return `${PANEL_PREFIX}:${userId || 'signed-out'}:${setting}`;
}

export function readSetting(key: string): string | null {
  try {
    return localStorage.getItem(key);
  } catch {
    return null;
  }
}

export function writeSetting(key: string, value: string): void {
  try {
    localStorage.setItem(key, value);
  } catch {
    return;
  }
}

export function readFlag(key: string): boolean {
  return readSetting(key) === 'true';
}

export function clampWidth(width: number, max: number = PANEL_WIDTH.max): number {
  const ceiling = Math.max(PANEL_WIDTH.min, Math.min(PANEL_WIDTH.max, max));
  return Math.round(Math.min(ceiling, Math.max(PANEL_WIDTH.min, width)));
}

export function readWidth(key: string): number {
  const stored = Number(readSetting(key));
  return Number.isFinite(stored) && stored > 0 ? clampWidth(stored) : PANEL_WIDTH.fallback;
}
