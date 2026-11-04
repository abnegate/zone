import { beforeEach, describe, expect, it } from 'bun:test';
import {
  DEFERRED_STYLESHEET_ATTRIBUTE,
  FEATURE_STYLESHEET_ATTRIBUTE,
  FONT_STYLESHEET,
  loadFeatureStylesheet,
  loadFonts,
  resetFeatureStylesheet,
  stylesheetsLoaded,
} from './stylesheet';

function page(...medias: string[]) {
  const root = document.createElement('div');
  const links = medias.map((media) => {
    const link = document.createElement('link');
    link.setAttribute(DEFERRED_STYLESHEET_ATTRIBUTE, '');
    link.media = media;
    root.append(link);
    return link;
  });
  return { root, links };
}

async function settled(promise: Promise<void>): Promise<boolean> {
  let done = false;
  void promise.then(() => {
    done = true;
  });
  await new Promise((resolve) => setTimeout(resolve));
  return done;
}

describe('stylesheetsLoaded', () => {
  it('resolves at once without a deferred stylesheet', async () => {
    expect(await settled(stylesheetsLoaded(page().root))).toBe(true);
  });

  it('resolves at once when the stylesheets have already applied', async () => {
    expect(await settled(stylesheetsLoaded(page('all', 'all').root))).toBe(true);
  });

  it('waits for every stylesheet still loading', async () => {
    const { root, links } = page('print', 'all', 'print');
    const loading = stylesheetsLoaded(root);
    links[0].dispatchEvent(new Event('load'));
    expect(await settled(loading)).toBe(false);
    links[2].dispatchEvent(new Event('load'));
    expect(await settled(loading)).toBe(true);
  });

  it('still renders when a stylesheet fails to load', async () => {
    const { root, links } = page('print');
    const loading = stylesheetsLoaded(root);
    links[0].dispatchEvent(new Event('error'));
    expect(await settled(loading)).toBe(true);
  });
});

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
