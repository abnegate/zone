import { afterEach, beforeEach, describe, expect, it } from 'bun:test';
import { fireEvent, render, screen } from '@testing-library/react';
import SubNav from './SubNav';

const KEY = 'manager_subnav_test';

function renderNav(defaultWidthRem?: number) {
  render(
    <SubNav storageKey={KEY} label="Resize list" defaultWidthRem={defaultWidthRem}>
      <span>List</span>
    </SubNav>
  );
  return {
    pane: document.querySelector('.sub-nav') as HTMLElement,
    handle: screen.getByRole('separator', { name: 'Resize list' }),
  };
}

beforeEach(() => {
  localStorage.removeItem(KEY);
});

afterEach(() => {
  document.documentElement.classList.remove('sub-nav-resizing');
  localStorage.removeItem(KEY);
});

describe('SubNav', () => {
  it('uses the default width when nothing is stored', () => {
    const { pane } = renderNav();
    expect(pane).toHaveStyle({ '--sub-nav-width': '288px' });
  });

  it('honours a per-pane default in rem', () => {
    const { pane } = renderNav(20);
    expect(pane).toHaveStyle({ '--sub-nav-width': '320px' });
  });

  it('restores a stored width', () => {
    localStorage.setItem(KEY, '360');
    const { pane } = renderNav();
    expect(pane).toHaveStyle({ '--sub-nav-width': '360px' });
  });

  it('ignores a stored value that is not a number', () => {
    localStorage.setItem(KEY, 'wide');
    const { pane } = renderNav();
    expect(pane).toHaveStyle({ '--sub-nav-width': '288px' });
  });

  it('clamps a stored width to the min', () => {
    localStorage.setItem(KEY, '10');
    const { pane } = renderNav();
    expect(pane).toHaveStyle({ '--sub-nav-width': '192px' });
  });

  it('clamps a stored width to the max', () => {
    localStorage.setItem(KEY, '9999');
    const { pane } = renderNav();
    expect(pane).toHaveStyle({ '--sub-nav-width': '640px' });
  });

  it('widens on pointer drag and persists on release', () => {
    const { pane, handle } = renderNav();
    fireEvent.pointerDown(handle, { clientX: 288, button: 0 });
    expect(document.documentElement.classList.contains('sub-nav-resizing')).toBe(true);
    fireEvent.pointerMove(window, { clientX: 348 });
    expect(pane).toHaveStyle({ '--sub-nav-width': '348px' });
    fireEvent.pointerUp(window);
    expect(localStorage.getItem(KEY)).toBe('348');
    expect(document.documentElement.classList.contains('sub-nav-resizing')).toBe(false);
    expect(pane.classList.contains('resizing')).toBe(false);
  });

  it('narrows with ArrowLeft and persists', () => {
    const { pane, handle } = renderNav();
    fireEvent.keyDown(handle, { key: 'ArrowLeft' });
    expect(pane).toHaveStyle({ '--sub-nav-width': '272px' });
    expect(localStorage.getItem(KEY)).toBe('272');
  });

  it('widens faster with Shift+ArrowRight', () => {
    const { pane, handle } = renderNav();
    fireEvent.keyDown(handle, { key: 'ArrowRight', shiftKey: true });
    expect(pane).toHaveStyle({ '--sub-nav-width': '320px' });
  });

  it('jumps to the min on Home', () => {
    const { pane, handle } = renderNav();
    fireEvent.keyDown(handle, { key: 'Home' });
    expect(pane).toHaveStyle({ '--sub-nav-width': '192px' });
  });

  it('jumps to the max on End', () => {
    const { pane, handle } = renderNav();
    fireEvent.keyDown(handle, { key: 'End' });
    expect(pane).toHaveStyle({ '--sub-nav-width': '640px' });
  });

  it('resets to the default on double click', () => {
    localStorage.setItem(KEY, '400');
    const { pane, handle } = renderNav();
    fireEvent.doubleClick(handle);
    expect(pane).toHaveStyle({ '--sub-nav-width': '288px' });
    expect(localStorage.getItem(KEY)).toBe('288');
  });
});
