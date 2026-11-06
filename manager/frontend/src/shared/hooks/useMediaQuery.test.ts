import { afterEach, beforeEach, describe, expect, it } from 'bun:test';
import { act, renderHook } from '@testing-library/react';
import { useMediaQuery } from './useMediaQuery';

type ChangeListener = (event: MediaQueryListEvent) => void;

class StubMediaQueryList {
  readonly listeners = new Set<ChangeListener>();
  onchange: ChangeListener | null = null;

  constructor(
    readonly media: string,
    public matches: boolean
  ) {}

  addEventListener(type: string, listener: ChangeListener): void {
    if (type === 'change') {
      this.listeners.add(listener);
    }
  }

  removeEventListener(type: string, listener: ChangeListener): void {
    if (type === 'change') {
      this.listeners.delete(listener);
    }
  }

  addListener(listener: ChangeListener): void {
    this.listeners.add(listener);
  }

  removeListener(listener: ChangeListener): void {
    this.listeners.delete(listener);
  }

  dispatchEvent(): boolean {
    return false;
  }

  change(matches: boolean): void {
    this.matches = matches;
    const event = { matches, media: this.media } as MediaQueryListEvent;
    for (const listener of [...this.listeners]) {
      listener(event);
    }
  }
}

const QUERY = '(max-width: 768px)';
const OTHER_QUERY = '(min-width: 1200px)';

const originalMatchMedia = window.matchMedia;
let lists: Map<string, StubMediaQueryList>;
let initialMatches: Map<string, boolean>;

const listFor = (query: string): StubMediaQueryList => {
  const existing = lists.get(query);
  if (existing) {
    return existing;
  }
  const list = new StubMediaQueryList(query, initialMatches.get(query) ?? false);
  lists.set(query, list);
  return list;
};

beforeEach(() => {
  lists = new Map();
  initialMatches = new Map();
  Object.defineProperty(window, 'matchMedia', {
    writable: true,
    configurable: true,
    value: (query: string) => listFor(query) as unknown as MediaQueryList,
  });
});

afterEach(() => {
  Object.defineProperty(window, 'matchMedia', {
    writable: true,
    configurable: true,
    value: originalMatchMedia,
  });
});

describe('useMediaQuery', () => {
  it('reads whether the query matches now', () => {
    initialMatches.set(QUERY, true);

    const { result } = renderHook(() => useMediaQuery(QUERY));

    expect(result.current).toBe(true);
  });

  it('reads false when the query does not match', () => {
    const { result } = renderHook(() => useMediaQuery(QUERY));

    expect(result.current).toBe(false);
  });

  it('follows change events in both directions', () => {
    const { result } = renderHook(() => useMediaQuery(QUERY));
    expect(result.current).toBe(false);

    act(() => listFor(QUERY).change(true));
    expect(result.current).toBe(true);

    act(() => listFor(QUERY).change(false));
    expect(result.current).toBe(false);
  });

  it('listens to the query it was given and no other', () => {
    initialMatches.set(OTHER_QUERY, true);
    const { result } = renderHook(() => useMediaQuery(QUERY));

    expect(listFor(QUERY).listeners.size, 'subscribes to its own query').toBeGreaterThan(0);
    expect(result.current).toBe(false);

    act(() => listFor(OTHER_QUERY).change(true));
    expect(result.current).toBe(false);
  });

  it('follows a new query when the argument changes', () => {
    initialMatches.set(OTHER_QUERY, true);
    const { result, rerender } = renderHook(({ query }) => useMediaQuery(query), {
      initialProps: { query: QUERY },
    });
    expect(result.current).toBe(false);

    rerender({ query: OTHER_QUERY });
    expect(result.current).toBe(true);

    act(() => listFor(OTHER_QUERY).change(false));
    expect(result.current).toBe(false);
    expect(listFor(QUERY).listeners.size, 'drops the old query listener').toBe(0);
  });

  it('stops listening once unmounted', () => {
    const { unmount } = renderHook(() => useMediaQuery(QUERY));
    expect(listFor(QUERY).listeners.size).toBeGreaterThan(0);

    unmount();

    expect(listFor(QUERY).listeners.size, 'removes every change listener').toBe(0);
  });
});
