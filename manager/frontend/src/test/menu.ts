import { act, fireEvent, screen } from '@testing-library/react';

export async function settleMenu(): Promise<void> {
  await act(() => new Promise((resolve) => setTimeout(resolve, 0)));
}

export async function openMenu(label: string | RegExp): Promise<HTMLElement> {
  const trigger = screen.getByRole('button', { name: label });
  fireEvent.keyDown(trigger, { key: 'Enter' });
  await settleMenu();
  return trigger;
}

export async function chooseMenuItem(
  label: string | RegExp,
  itemName: string | RegExp
): Promise<HTMLElement> {
  const trigger = await openMenu(label);
  fireEvent.click(screen.getByRole('menuitem', { name: itemName }));
  await settleMenu();
  return trigger;
}
