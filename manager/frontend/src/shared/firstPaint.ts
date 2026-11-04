export const FIRST_PAINT_FALLBACK_MILLISECONDS = 1000;

const FIRST_CONTENTFUL_PAINT = 'first-contentful-paint';

type Observer = Pick<typeof PerformanceObserver, 'supportedEntryTypes'> &
  (new (
    callback: PerformanceObserverCallback
  ) => Pick<PerformanceObserver, 'observe' | 'disconnect'>);

export interface FirstPaintEnvironment {
  document: Pick<Document, 'hidden'>;
  observer: Observer | undefined;
  frame: (callback: () => void) => void;
  wait: (callback: () => void, milliseconds: number) => ReturnType<typeof setTimeout>;
  cancel: (timer: ReturnType<typeof setTimeout>) => void;
}

const browser: FirstPaintEnvironment = {
  document,
  observer: globalThis.PerformanceObserver,
  frame: (callback) => requestAnimationFrame(callback),
  wait: (callback, milliseconds) => setTimeout(callback, milliseconds),
  cancel: (timer) => clearTimeout(timer),
};

export function afterFirstContentfulPaint(
  start: () => void,
  environment: FirstPaintEnvironment = browser
): void {
  if (environment.document.hidden) {
    start();
    return;
  }
  const Observer = environment.observer;
  if (!Observer?.supportedEntryTypes?.includes('paint')) {
    environment.frame(() => environment.frame(start));
    return;
  }
  let started = false;
  const observer = new Observer((list) => {
    if (list.getEntriesByName(FIRST_CONTENTFUL_PAINT).length > 0) {
      once();
    }
  });
  const timer = environment.wait(() => once(), FIRST_PAINT_FALLBACK_MILLISECONDS);
  function once() {
    if (started) {
      return;
    }
    started = true;
    observer.disconnect();
    environment.cancel(timer);
    start();
  }
  observer.observe({ type: 'paint', buffered: true });
}
