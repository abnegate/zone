export const DEFERRED_STYLESHEET_ATTRIBUTE = 'data-deferred-stylesheet';

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
