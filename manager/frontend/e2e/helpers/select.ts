import { expect, type Locator } from '@playwright/test';

export async function chooseSelect(trigger: Locator, option: string): Promise<void> {
  await trigger.click();
  const listbox = trigger.page().getByRole('listbox');
  await listbox.getByRole('option', { name: option, exact: true }).click();
  await expect(listbox).toHaveCount(0);
  await expect(trigger).toHaveText(option);
}

export async function expectSelectOptions(trigger: Locator, options: string[]): Promise<void> {
  await trigger.click();
  const listbox = trigger.page().getByRole('listbox');
  await expect(listbox.getByRole('option')).toHaveText(options);
  await trigger.page().keyboard.press('Escape');
  await expect(listbox).toHaveCount(0);
}
