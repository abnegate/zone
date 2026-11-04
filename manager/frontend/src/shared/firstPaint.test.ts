import { describe, expect, it } from 'bun:test';
import {
  afterFirstContentfulPaint,
  FIRST_PAINT_FALLBACK_MILLISECONDS,
  type FirstPaintEnvironment,
} from './firstPaint';

function environment({ hidden = false, paint = true }: { hidden?: boolean; paint?: boolean } = {}) {
  const frames: (() => void)[] = [];
  const timers: { callback: () => void; milliseconds: number; cancelled: boolean }[] = [];
  const observers: { callback: PerformanceObserverCallback; disconnected: boolean }[] = [];

  class Observer {
    static supportedEntryTypes = paint ? ['paint', 'navigation'] : ['navigation'];
    private readonly record: (typeof observers)[number];

    constructor(callback: PerformanceObserverCallback) {
      this.record = { callback, disconnected: false };
      observers.push(this.record);
    }

    observe() {}

    disconnect() {
      this.record.disconnected = true;
    }
  }

  const fake: FirstPaintEnvironment = {
    document: { hidden },
    observer: Observer as unknown as FirstPaintEnvironment['observer'],
    frame: (callback) => {
      frames.push(callback);
    },
    wait: (callback, milliseconds) => {
      timers.push({ callback, milliseconds, cancelled: false });
      return (timers.length - 1) as unknown as ReturnType<typeof setTimeout>;
    },
    cancel: (timer) => {
      timers[timer as unknown as number].cancelled = true;
    },
  };

  function paintEntries(...names: string[]) {
    const list = {
      getEntriesByName: (name: string) => names.filter((entry) => entry === name),
    } as unknown as PerformanceObserverEntryList;
    for (const observer of observers) {
      observer.callback(list, {} as PerformanceObserver);
    }
  }

  return { fake, frames, timers, observers, paintEntries };
}

describe('afterFirstContentfulPaint', () => {
  it('starts at once in a hidden tab, which never paints', () => {
    const { fake } = environment({ hidden: true });
    let started = 0;
    afterFirstContentfulPaint(() => {
      started += 1;
    }, fake);
    expect(started).toBe(1);
  });

  it('waits for the first contentful paint, not the first paint', () => {
    const { fake, paintEntries, observers, timers } = environment();
    let started = 0;
    afterFirstContentfulPaint(() => {
      started += 1;
    }, fake);
    paintEntries('first-paint');
    expect(started).toBe(0);
    paintEntries('first-paint', 'first-contentful-paint');
    expect(started).toBe(1);
    expect(observers[0].disconnected).toBe(true);
    expect(timers[0].cancelled).toBe(true);
  });

  it('starts once even if the paint is reported again', () => {
    const { fake, paintEntries } = environment();
    let started = 0;
    afterFirstContentfulPaint(() => {
      started += 1;
    }, fake);
    paintEntries('first-contentful-paint');
    paintEntries('first-contentful-paint');
    expect(started).toBe(1);
  });

  it('starts anyway when no contentful paint is reported in time', () => {
    const { fake, timers, observers } = environment();
    let started = 0;
    afterFirstContentfulPaint(() => {
      started += 1;
    }, fake);
    expect(timers[0].milliseconds).toBe(FIRST_PAINT_FALLBACK_MILLISECONDS);
    timers[0].callback();
    expect(started).toBe(1);
    expect(observers[0].disconnected).toBe(true);
  });

  it('waits two frames where paint timing is unsupported', () => {
    const { fake, frames } = environment({ paint: false });
    let started = 0;
    afterFirstContentfulPaint(() => {
      started += 1;
    }, fake);
    frames.shift()?.();
    expect(started).toBe(0);
    frames.shift()?.();
    expect(started).toBe(1);
  });
});
