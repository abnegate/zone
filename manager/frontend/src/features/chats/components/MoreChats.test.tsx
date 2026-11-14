import { afterEach, describe, expect, it, mock } from 'bun:test';
import { act, fireEvent, render, screen } from '@testing-library/react';
import { LOAD_MORE_CHATS, LOADING_MORE_CHATS, MoreChats } from './MoreChats';

type Intersect = (entries: Array<{ isIntersecting: boolean }>) => void;

const original = globalThis.IntersectionObserver;
let observers: Array<{ callback: Intersect; disconnected: boolean }> = [];

class FakeObserver {
  private readonly record: { callback: Intersect; disconnected: boolean };
  constructor(callback: Intersect) {
    this.record = { callback, disconnected: false };
    observers.push(this.record);
  }
  observe() {}
  disconnect() {
    this.record.disconnected = true;
  }
}

const watch = () => {
  observers = [];
  globalThis.IntersectionObserver = FakeObserver as unknown as typeof IntersectionObserver;
};

const live = () => observers.filter((observer) => !observer.disconnected);

afterEach(() => {
  globalThis.IntersectionObserver = original;
});

describe('MoreChats', () => {
  it('loads the next page when clicked and says how many are left', () => {
    const load = mock();
    render(<MoreChats remaining={12} loading={false} onLoad={load} />);

    fireEvent.click(screen.getByRole('button', { name: `${LOAD_MORE_CHATS} (12)` }));

    expect(load).toHaveBeenCalledTimes(1);
  });

  it('cannot be clicked again while a page is loading', () => {
    render(<MoreChats remaining={12} loading onLoad={mock()} />);

    expect(screen.getByRole('button', { name: LOADING_MORE_CHATS })).toBeDisabled();
  });

  it('loads the next page when the end of the list scrolls into view', () => {
    watch();
    const load = mock();
    render(<MoreChats remaining={12} loading={false} onLoad={load} />);

    act(() => live()[0]?.callback([{ isIntersecting: true }]));

    expect(load).toHaveBeenCalledTimes(1);
  });

  it('stops watching while a page loads and looks again once it has', () => {
    watch();
    const { rerender } = render(<MoreChats remaining={12} loading={false} onLoad={mock()} />);

    rerender(<MoreChats remaining={12} loading onLoad={mock()} />);
    expect(live()).toHaveLength(0);

    rerender(<MoreChats remaining={4} loading={false} onLoad={mock()} />);
    expect(live()).toHaveLength(1);
  });
});
