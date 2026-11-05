export const VIEWPORT_MARGIN = 16;

export interface Span {
  left: number;
  right: number;
}

export function popoverOffset(trigger: Span, width: number, viewport: number): number {
  const fits = trigger.left + width <= viewport - VIEWPORT_MARGIN;
  const preferred = fits ? trigger.left : trigger.right - width;
  const left = Math.max(VIEWPORT_MARGIN, Math.min(preferred, viewport - VIEWPORT_MARGIN - width));
  return left - trigger.left;
}
