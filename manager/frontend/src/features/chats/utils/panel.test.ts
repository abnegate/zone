import { afterEach, beforeEach, describe, expect, it, mock, spyOn } from 'bun:test';
import { clampWidth, PANEL_WIDTH, panelKey, readFlag, readWidth, writeSetting } from './panel';

describe('panel settings', () => {
  beforeEach(() => {
    localStorage.clear();
  });

  afterEach(() => {
    mock.restore();
  });

  it('keeps each viewer to their own settings', () => {
    expect(panelKey('user-1', 'width')).toBe('zone:chat-panel:user-1:width');
    expect(panelKey(null, 'gallery')).toBe('zone:chat-panel:signed-out:gallery');
  });

  it('keeps the width between its bounds and below the space it is given', () => {
    expect(clampWidth(10)).toBe(PANEL_WIDTH.min);
    expect(clampWidth(10_000)).toBe(PANEL_WIDTH.max);
    expect(clampWidth(500, 400)).toBe(400);
    expect(clampWidth(500, 100)).toBe(PANEL_WIDTH.min);
  });

  it('reads back a stored width, and falls back on one it cannot use', () => {
    writeSetting('width', '400');
    expect(readWidth('width')).toBe(400);
    for (const stored of ['wide', '-3', '0']) {
      writeSetting('width', stored);
      expect(readWidth('width')).toBe(PANEL_WIDTH.fallback);
    }
    expect(readWidth('missing')).toBe(PANEL_WIDTH.fallback);
  });

  it('carries on with the defaults when the browser refuses storage', () => {
    spyOn(localStorage, 'getItem').mockImplementation(() => {
      throw new Error('denied');
    });
    spyOn(localStorage, 'setItem').mockImplementation(() => {
      throw new Error('denied');
    });

    expect(() => writeSetting('collapsed', 'true')).not.toThrow();
    expect(readFlag('collapsed')).toBe(false);
    expect(readWidth('width')).toBe(PANEL_WIDTH.fallback);
  });
});
