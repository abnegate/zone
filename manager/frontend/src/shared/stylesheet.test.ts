import { describe, expect, it } from 'bun:test';
import {
  DEFERRED_STYLESHEET_ATTRIBUTE,
  FONT_STYLESHEET,
  loadFonts,
  stylesheetLoaded,
} from './stylesheet';

function page(media?: string) {
  const root = document.createElement('div');
  if (media === undefined) {
    return { root, link: null };
  }
  const link = document.createElement('link');
  link.setAttribute(DEFERRED_STYLESHEET_ATTRIBUTE, '');
  link.media = media;
  root.append(link);
  return { root, link };
}

async function settled(promise: Promise<void>): Promise<boolean> {
  let done = false;
  void promise.then(() => {
    done = true;
  });
  await new Promise((resolve) => setTimeout(resolve));
  return done;
}

describe('stylesheetLoaded', () => {
  it('resolves at once without a deferred stylesheet', async () => {
    expect(await settled(stylesheetLoaded(page().root))).toBe(true);
  });

  it('resolves at once when the stylesheet has already applied', async () => {
    expect(await settled(stylesheetLoaded(page('all').root))).toBe(true);
  });

  it('waits for the stylesheet to load', async () => {
    const { root, link } = page('print');
    const loaded = stylesheetLoaded(root);
    expect(await settled(loaded)).toBe(false);
    link?.dispatchEvent(new Event('load'));
    expect(await settled(loaded)).toBe(true);
  });

  it('still renders when the stylesheet fails to load', async () => {
    const { root, link } = page('print');
    const loaded = stylesheetLoaded(root);
    link?.dispatchEvent(new Event('error'));
    expect(await settled(loaded)).toBe(true);
  });
});

describe('loadFonts', () => {
  it('appends the font stylesheet to the head', () => {
    const target = document.implementation.createHTMLDocument();
    loadFonts(target);
    const link = target.head.querySelector('link');
    expect(link?.rel).toBe('stylesheet');
    expect(link?.getAttribute('href')).toBe(FONT_STYLESHEET);
  });
});
