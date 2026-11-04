export const DEFERRED_STYLESHEET_ATTRIBUTE = 'data-deferred-stylesheet';

export const FONT_STYLESHEET =
  'https://fonts.googleapis.com/css2?family=IBM+Plex+Mono:wght@400;500;600&family=Newsreader:opsz,wght@6..72,500;6..72,600;6..72,700&family=Source+Sans+3:ital,wght@0,400;0,500;0,600;0,700;1,400&family=Inter:wght@400;500;600;700&family=Roboto:wght@400;500;600;700&family=Open+Sans:wght@400;500;600;700&family=Lato:wght@400;700&family=Nunito:wght@400;500;600;700&display=swap';

export function stylesheetLoaded(root: ParentNode = document): Promise<void> {
  const link = root.querySelector<HTMLLinkElement>(`link[${DEFERRED_STYLESHEET_ATTRIBUTE}]`);
  if (link?.media !== 'print') {
    return Promise.resolve();
  }
  return new Promise((resolve) => {
    link.addEventListener('load', () => resolve(), { once: true });
    link.addEventListener('error', () => resolve(), { once: true });
  });
}

export function loadFonts(target: Document = document): void {
  const link = target.createElement('link');
  link.rel = 'stylesheet';
  link.href = FONT_STYLESHEET;
  target.head.append(link);
}
