export const FEATURE_STYLESHEET_ATTRIBUTE = 'data-feature-stylesheet';
export const FONT_STYLESHEET_ATTRIBUTE = 'data-font-stylesheet';

function loaded(link: HTMLLinkElement): Promise<void> {
  return new Promise((resolve) => {
    link.addEventListener('load', () => resolve(), { once: true });
    link.addEventListener('error', () => resolve(), { once: true });
  });
}

let featureStylesheet: Promise<void> | null = null;

export function resetFeatureStylesheet(): void {
  featureStylesheet = null;
}

export function loadFeatureStylesheet(root: ParentNode = document): Promise<void> {
  if (!featureStylesheet) {
    const link = root.querySelector<HTMLLinkElement>(`link[${FEATURE_STYLESHEET_ATTRIBUTE}]`);
    const href = link?.getAttribute(FEATURE_STYLESHEET_ATTRIBUTE);
    if (link && href) {
      featureStylesheet = loaded(link);
      link.href = href;
    } else {
      featureStylesheet = Promise.resolve();
    }
  }
  return featureStylesheet;
}

export function loadFonts(href: string, target: Document = document): void {
  const link = target.createElement('link');
  link.rel = 'stylesheet';
  link.href = href;
  link.setAttribute(FONT_STYLESHEET_ATTRIBUTE, '');
  target.head.append(link);
}
