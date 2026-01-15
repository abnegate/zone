import type { Page } from '@playwright/test';

export const selectOption = async (page: Page, label: string, optionLabel: string) => {
  const trigger = page.getByLabel(label);
  await trigger.click();
  await page.getByRole('option', { name: optionLabel }).click();
};
