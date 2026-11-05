export const VIEWPORT_MARGIN = 16;

export function popoverOffset(
  trigger: Pick<DOMRect, 'left' | 'right'>,
  width: number,
  viewport: number,
  margin: number = VIEWPORT_MARGIN
): number {
  const fits = trigger.left + width <= viewport - margin;
  const preferred = fits ? trigger.left : trigger.right - width;
  const left = Math.max(margin, Math.min(preferred, viewport - margin - width));
  return left - trigger.left;
}

export function viewportMargin(element: Element): number {
  const style = getComputedStyle(element);
  const gutter = style.getPropertyValue('--ui-gutter').trim();
  const size = Number.parseFloat(gutter);
  if (Number.isNaN(size)) return VIEWPORT_MARGIN;
  if (!gutter.endsWith('rem')) return size;
  return size * Number.parseFloat(getComputedStyle(document.documentElement).fontSize);
}
