import { beforeEach, describe, expect, it } from 'bun:test';
import {
  FEATURE_STYLESHEET_ATTRIBUTE,
  FONT_STYLESHEET,
  loadFeatureStylesheet,
  loadFonts,
  resetFeatureStylesheet,
} from './stylesheet';

async function settled(promise: Promise<void>): Promise<boolean> {
  let done = false;
  void promise.then(() => {
    done = true;
  });
  await new Promise((resolve) => setTimeout(resolve));
  return done;
}

describe('loadFeatureStylesheet', () => {
  function featurePage() {
    const root = document.createElement('div');
    const link = document.createElement('link');
    link.setAttribute(FEATURE_STYLESHEET_ATTRIBUTE, '/assets/styles-features.css');
    root.append(link);
    return { root, link };
  }

  beforeEach(() => {
    resetFeatureStylesheet();
  });

  it('fetches only when asked and resolves once loaded', async () => {
    const { root, link } = featurePage();
    expect(link.getAttribute('href')).toBeNull();
    const loading = loadFeatureStylesheet(root);
    expect(link.getAttribute('href')).toBe('/assets/styles-features.css');
    expect(await settled(loading)).toBe(false);
    link.dispatchEvent(new Event('load'));
    expect(await settled(loading)).toBe(true);
  });

  it('shares one load between callers', () => {
    const { root } = featurePage();
    expect(loadFeatureStylesheet(root)).toBe(loadFeatureStylesheet(root));
  });

  it('resolves at once without the link, as in development', async () => {
    expect(await settled(loadFeatureStylesheet(document.createElement('div')))).toBe(true);
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
