import { describe, expect, it, mock } from 'bun:test';
import { act, renderHook } from '@testing-library/react';
import { usePageRestore } from './usePageRestore';

function showPage(persisted: boolean) {
  act(() => {
    window.dispatchEvent(Object.assign(new Event('pageshow'), { persisted }));
  });
}

describe('usePageRestore', () => {
  it('runs the callback when the page is restored from the back-forward cache', () => {
    const onRestore = mock();
    renderHook(() => usePageRestore(onRestore));

    showPage(true);

    expect(onRestore).toHaveBeenCalledTimes(1);
  });

  it('ignores a page that was freshly loaded', () => {
    const onRestore = mock();
    renderHook(() => usePageRestore(onRestore));

    showPage(false);

    expect(onRestore).not.toHaveBeenCalled();
  });

  it('runs the latest callback it was given', () => {
    const first = mock();
    const second = mock();
    const { rerender } = renderHook(({ onRestore }) => usePageRestore(onRestore), {
      initialProps: { onRestore: first },
    });

    rerender({ onRestore: second });
    showPage(true);

    expect(first).not.toHaveBeenCalled();
    expect(second).toHaveBeenCalledTimes(1);
  });

  it('stops listening once unmounted', () => {
    const onRestore = mock();
    const { unmount } = renderHook(() => usePageRestore(onRestore));

    unmount();
    showPage(true);

    expect(onRestore).not.toHaveBeenCalled();
  });
});
