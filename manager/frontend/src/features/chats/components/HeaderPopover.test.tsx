import { describe, expect, it, mock } from 'bun:test';
import { act, fireEvent, render, screen } from '@testing-library/react';
import { HeaderPopover } from './HeaderPopover';
import { ModelNameInput } from './ModelNameInput';

function renderPopover(disabled = false) {
  return render(
    <div>
      <button type="button">Elsewhere</button>
      <HeaderPopover
        className="pill"
        testId="trigger"
        label="Settings"
        triggerLabel="Open settings"
        title="Settings"
        disabled={disabled}
        trigger={<span>Summary</span>}
      >
        <button type="button">Inside</button>
      </HeaderPopover>
    </div>
  );
}

function selectMenu(): HTMLElement {
  const menu = document.createElement('div');
  menu.setAttribute('data-radix-popper-content-wrapper', '');
  document.body.append(menu);
  return menu;
}

function openSelectInside(): HTMLElement {
  const select = document.createElement('button');
  select.setAttribute('role', 'combobox');
  select.setAttribute('aria-expanded', 'true');
  screen.getByRole('dialog').append(select);
  return select;
}

describe('HeaderPopover', () => {
  it('names itself and hides its panel until opened', () => {
    renderPopover();
    const trigger = screen.getByRole('button', { name: 'Open settings' });
    expect(trigger).toHaveAttribute('aria-expanded', 'false');
    expect(trigger).toHaveAttribute('aria-haspopup', 'dialog');
    expect(screen.queryByRole('dialog')).toBeNull();
    expect(trigger.getAttribute('aria-controls')).toBe(
      document.querySelector('.chat-header-popover-panel')?.id ?? null
    );
  });

  it('opens on click and moves focus into the panel', () => {
    renderPopover();
    fireEvent.click(screen.getByTestId('trigger'));
    expect(document.activeElement).toBe(screen.getByRole('dialog', { name: 'Settings' }));
  });

  it('closes on a second click', () => {
    renderPopover();
    const trigger = screen.getByTestId('trigger');
    fireEvent.click(trigger);
    fireEvent.click(trigger);
    expect(trigger).toHaveAttribute('aria-expanded', 'false');
  });

  it('closes on Escape and hands focus back to the trigger', () => {
    renderPopover();
    const trigger = screen.getByTestId('trigger');
    fireEvent.click(trigger);
    fireEvent.keyDown(document.body, { key: 'Escape' });
    expect(trigger).toHaveAttribute('aria-expanded', 'false');
    expect(document.activeElement).toBe(trigger);
  });

  it('closes on a press outside and stays open on a press inside', () => {
    renderPopover();
    const trigger = screen.getByTestId('trigger');
    fireEvent.click(trigger);
    fireEvent.pointerDown(screen.getByRole('button', { name: 'Inside' }));
    expect(trigger).toHaveAttribute('aria-expanded', 'true');
    fireEvent.pointerDown(screen.getByRole('button', { name: 'Elsewhere' }));
    expect(trigger).toHaveAttribute('aria-expanded', 'false');
  });

  it('leaves Escape and outside presses to a select menu open over it', () => {
    renderPopover();
    const trigger = screen.getByTestId('trigger');
    fireEvent.click(trigger);
    const select = openSelectInside();
    const menu = selectMenu();
    try {
      fireEvent.keyDown(menu, { key: 'Escape' });
      fireEvent.pointerDown(menu);
      fireEvent.pointerDown(screen.getByRole('button', { name: 'Elsewhere' }));
      expect(trigger).toHaveAttribute('aria-expanded', 'true');

      select.setAttribute('aria-expanded', 'false');
      fireEvent.keyDown(menu, { key: 'Escape' });
      expect(trigger).toHaveAttribute('aria-expanded', 'true');
    } finally {
      menu.remove();
    }
    fireEvent.keyDown(document.body, { key: 'Escape' });
    expect(trigger).toHaveAttribute('aria-expanded', 'false');
  });

  it('closes when focus moves out, but not into a select menu', () => {
    renderPopover();
    const trigger = screen.getByTestId('trigger');
    const inside = screen.getByRole('button', { name: 'Inside', hidden: true });
    fireEvent.click(trigger);
    const menu = selectMenu();
    try {
      fireEvent.focusOut(inside, { relatedTarget: menu });
      fireEvent.focusOut(inside, { relatedTarget: trigger });
      fireEvent.focusOut(inside, { relatedTarget: null });
      expect(trigger).toHaveAttribute('aria-expanded', 'true');
    } finally {
      menu.remove();
    }
    fireEvent.focusOut(inside, {
      relatedTarget: screen.getByRole('button', { name: 'Elsewhere' }),
    });
    expect(trigger).toHaveAttribute('aria-expanded', 'false');
  });

  it('closes on Escape from a half-typed model name without choosing it', () => {
    const onSubmit = mock();
    render(
      <HeaderPopover
        className="pill"
        testId="trigger"
        label="Settings"
        triggerLabel="Open settings"
        title="Settings"
        trigger={<span>Summary</span>}
      >
        <ModelNameInput provider="OpenAI" value="" onSubmit={onSubmit} />
      </HeaderPopover>
    );
    const trigger = screen.getByTestId('trigger');
    fireEvent.click(trigger);
    const field = screen.getByRole('textbox', { name: 'Provider model name' });
    field.focus();

    fireEvent.change(field, { target: { value: 'gpt-' } });
    fireEvent.keyDown(field, { key: 'Escape' });

    expect(trigger).toHaveAttribute('aria-expanded', 'false');
    expect(document.activeElement).toBe(trigger);
    expect(onSubmit).not.toHaveBeenCalled();
  });

  it('stays shut while disabled', () => {
    renderPopover(true);
    const trigger = screen.getByTestId('trigger');
    fireEvent.click(trigger);
    expect(trigger).toBeDisabled();
    expect(screen.queryByRole('dialog')).toBeNull();
  });

  it('opens under its own trigger and flips left only to stay on screen', () => {
    renderPopover();
    const anchor = document.querySelector<HTMLElement>('.chat-header-popover')!;
    const panel = document.querySelector<HTMLElement>('.chat-header-popover-panel')!;
    Object.defineProperty(document.documentElement, 'clientWidth', {
      configurable: true,
      value: 1280,
    });
    Object.defineProperty(panel, 'offsetWidth', { configurable: true, value: 320 });
    const place = (left: number, right: number) => {
      anchor.getBoundingClientRect = () => ({ left, right }) as DOMRect;
    };
    try {
      place(500, 700);
      fireEvent.click(screen.getByTestId('trigger'));
      expect(panel.style.left).toBe('0px');
      fireEvent.click(screen.getByTestId('trigger'));

      place(1000, 1260);
      fireEvent.click(screen.getByTestId('trigger'));
      expect(panel.style.left).toBe('-60px');
    } finally {
      Reflect.deleteProperty(document.documentElement, 'clientWidth');
    }
  });

  it('re-places an open panel when its row moves and stops once closed', () => {
    const observed: Element[] = [];
    const callbacks: ResizeObserverCallback[] = [];
    let disconnected = 0;
    const original = globalThis.ResizeObserver;
    globalThis.ResizeObserver = class {
      constructor(callback: ResizeObserverCallback) {
        callbacks.push(callback);
      }
      observe(element: Element) {
        observed.push(element);
      }
      unobserve() {}
      disconnect() {
        disconnected += 1;
      }
    } as unknown as typeof ResizeObserver;
    renderPopover();
    const anchor = document.querySelector<HTMLElement>('.chat-header-popover')!;
    const panel = document.querySelector<HTMLElement>('.chat-header-popover-panel')!;
    Object.defineProperty(document.documentElement, 'clientWidth', {
      configurable: true,
      value: 1280,
    });
    Object.defineProperty(panel, 'offsetWidth', { configurable: true, value: 320 });
    const place = (left: number, right: number) => {
      anchor.getBoundingClientRect = () => ({ left, right }) as DOMRect;
    };
    try {
      place(500, 700);
      fireEvent.click(screen.getByTestId('trigger'));
      expect(observed).toEqual([anchor, anchor.parentElement!]);

      place(1000, 1260);
      act(() => {
        for (const callback of callbacks) callback([], {} as ResizeObserver);
      });
      expect(panel.style.left).toBe('-60px');

      place(500, 700);
      fireEvent(window, new Event('resize'));
      expect(panel.style.left).toBe('0px');

      fireEvent.click(screen.getByTestId('trigger'));
      expect(disconnected).toBe(1);
      place(1000, 1260);
      fireEvent(window, new Event('resize'));
      expect(panel.style.left).toBe('0px');
    } finally {
      globalThis.ResizeObserver = original;
      Reflect.deleteProperty(document.documentElement, 'clientWidth');
    }
  });
});
