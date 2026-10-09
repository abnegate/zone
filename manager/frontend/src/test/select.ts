import { fireEvent, screen } from '@testing-library/react';

export function openSelect(label: string | RegExp): HTMLElement {
  const trigger = screen.getByRole('combobox', { name: label });
  fireEvent.keyDown(trigger, { key: 'ArrowDown' });
  return trigger;
}

export function chooseSelect(label: string | RegExp, optionName: string | RegExp): HTMLElement {
  const trigger = openSelect(label);
  fireEvent.click(screen.getByRole('option', { name: optionName }));
  return trigger;
}

export function chooseSelectElement(trigger: HTMLElement, optionName: string | RegExp): void {
  fireEvent.keyDown(trigger, { key: 'ArrowDown' });
  fireEvent.click(screen.getByRole('option', { name: optionName }));
}

export function selectOptionValues(label: string | RegExp): string[] {
  const trigger = openSelect(label);
  const values = screen
    .getAllByRole('option')
    .map((option) => option.getAttribute('data-value') ?? '');
  fireEvent.keyDown(trigger, { key: 'Escape' });
  return values;
}
