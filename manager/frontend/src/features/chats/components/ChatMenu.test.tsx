import { describe, expect, it, mock } from 'bun:test';
import { fireEvent, render, screen } from '@testing-library/react';
import { chooseMenuItem, openMenu, settleMenu } from '../../../test/menu';
import type { Chat } from '../types';
import { ChatMenu } from './ChatMenu';

const chat: Chat = {
  id: 'chat-1',
  title: 'Garden plans',
  model_name: 'llama3.2',
  created_at: '2024-01-01T00:00:00Z',
  updated_at: '2024-01-01T00:00:00Z',
  archived: false,
  agent_enabled: false,
};

const LABEL = 'Actions for Garden plans';

function renderMenu(archived = false) {
  const handlers = {
    onRename: mock(),
    onArchive: mock(),
    onUnarchive: mock(),
    onDelete: mock(),
  };
  const select = mock();
  const press = mock();
  render(
    // biome-ignore lint/a11y/noStaticElementInteractions: stands in for the clickable chat row
    <div data-testid="row" onClick={select} onKeyDown={(event) => press(event.key)}>
      <ChatMenu chat={chat} archived={archived} {...handlers} />
    </div>
  );
  return { handlers, select, press };
}

describe('ChatMenu', () => {
  it('labels one closed trigger per chat as a menu button', () => {
    renderMenu();
    const trigger = screen.getByRole('button', { name: LABEL });
    expect(trigger).toHaveAttribute('aria-haspopup', 'menu');
    expect(trigger).toHaveAttribute('aria-expanded', 'false');
    expect(screen.queryByRole('menu')).not.toBeInTheDocument();
  });

  it('opens from the keyboard with Rename, Archive and a destructive Delete', async () => {
    renderMenu();
    const trigger = await openMenu(LABEL);
    expect(trigger).toHaveAttribute('aria-expanded', 'true');
    const items = screen.getAllByRole('menuitem');
    expect(items.map((item) => item.textContent)).toEqual(['Rename', 'Archive', 'Delete']);
    expect(items[2]).toHaveClass('ui-action-menu-item-destructive');
    expect(items[0]).not.toHaveClass('ui-action-menu-item-destructive');
  });

  it('offers Unarchive instead of Archive in the archived view', async () => {
    const { handlers } = renderMenu(true);
    await openMenu(LABEL);
    expect(screen.queryByRole('menuitem', { name: 'Archive' })).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole('menuitem', { name: 'Unarchive' }));
    await settleMenu();
    expect(handlers.onUnarchive).toHaveBeenCalledWith(chat);
    expect(handlers.onArchive).not.toHaveBeenCalled();
  });

  for (const [item, handler] of [
    ['Rename', 'onRename'],
    ['Archive', 'onArchive'],
    ['Delete', 'onDelete'],
  ] as const) {
    it(`runs only ${handler} when ${item} is chosen, without selecting the row`, async () => {
      const { handlers, select } = renderMenu();
      const trigger = await chooseMenuItem(LABEL, item);
      for (const [name, fn] of Object.entries(handlers)) {
        expect(fn).toHaveBeenCalledTimes(name === handler ? 1 : 0);
      }
      expect(handlers[handler]).toHaveBeenCalledWith(chat);
      expect(select).not.toHaveBeenCalled();
      expect(screen.queryByRole('menu')).not.toBeInTheDocument();
      expect(document.activeElement).toBe(trigger);
    });
  }

  it('does not select the row when the trigger is clicked', async () => {
    const { select } = renderMenu();
    const trigger = screen.getByRole('button', { name: LABEL });
    fireEvent.pointerDown(trigger, { button: 0, ctrlKey: false });
    fireEvent.click(trigger);
    await settleMenu();
    expect(screen.getByRole('menu')).toBeInTheDocument();
    expect(select).not.toHaveBeenCalled();
  });

  it('keeps the keys that open the menu from the row and lets others through', () => {
    const { press } = renderMenu();
    const trigger = screen.getByRole('button', { name: LABEL });
    fireEvent.keyDown(trigger, { key: 'Tab' });
    expect(press).toHaveBeenCalledWith('Tab');
    for (const key of ['Enter', ' ', 'ArrowDown']) fireEvent.keyDown(trigger, { key });
    expect(press).toHaveBeenCalledTimes(1);
  });

  it('moves through the items with the arrow keys and activates with Enter', async () => {
    const { handlers } = renderMenu();
    await openMenu(LABEL);
    const menu = screen.getByRole('menu');
    const items = screen.getAllByRole('menuitem');
    fireEvent.keyDown(menu, { key: 'ArrowDown' });
    expect(document.activeElement).toBe(items[0]);
    fireEvent.keyDown(items[0], { key: 'ArrowDown' });
    await settleMenu();
    expect(document.activeElement).toBe(items[1]);
    fireEvent.keyDown(items[1], { key: 'Enter' });
    await settleMenu();
    expect(handlers.onArchive).toHaveBeenCalledWith(chat);
  });

  it('closes on Escape and returns focus to the trigger without running anything', async () => {
    const { handlers, select } = renderMenu();
    const trigger = await openMenu(LABEL);
    fireEvent.keyDown(screen.getByRole('menu'), { key: 'Escape' });
    await settleMenu();
    expect(screen.queryByRole('menu')).not.toBeInTheDocument();
    expect(trigger).toHaveAttribute('aria-expanded', 'false');
    expect(document.activeElement).toBe(trigger);
    for (const fn of Object.values(handlers)) expect(fn).not.toHaveBeenCalled();
    expect(select).not.toHaveBeenCalled();
  });
});
