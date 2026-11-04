import { describe, expect, it } from 'bun:test';
import { fireEvent, render, screen } from '@testing-library/react';
import { HeaderPopover } from './HeaderPopover';

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
    fireEvent.mouseDown(screen.getByRole('button', { name: 'Inside' }));
    expect(trigger).toHaveAttribute('aria-expanded', 'true');
    fireEvent.mouseDown(screen.getByRole('button', { name: 'Elsewhere' }));
    expect(trigger).toHaveAttribute('aria-expanded', 'false');
  });

  it('leaves Escape and outside presses to a select menu open over it', () => {
    renderPopover();
    const trigger = screen.getByTestId('trigger');
    fireEvent.click(trigger);
    const menu = selectMenu();
    try {
      fireEvent.keyDown(document.body, { key: 'Escape' });
      fireEvent.mouseDown(menu);
      expect(trigger).toHaveAttribute('aria-expanded', 'true');
    } finally {
      menu.remove();
    }
    fireEvent.keyDown(document.body, { key: 'Escape' });
    expect(trigger).toHaveAttribute('aria-expanded', 'false');
  });

  it('stays shut while disabled', () => {
    renderPopover(true);
    const trigger = screen.getByTestId('trigger');
    fireEvent.click(trigger);
    expect(trigger).toBeDisabled();
    expect(screen.queryByRole('dialog')).toBeNull();
  });
});
