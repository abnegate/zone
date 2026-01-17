import type { Page } from '@playwright/test';

export const selectOption = async (page: Page, label: string, optionLabel: string) => {
  const trigger = page.getByLabel(label);
  await trigger.click();
  await page.getByRole('option', { name: optionLabel }).click();
};

/**
 * Fill in all required security secrets by clicking the "Generate All Secrets" button.
 * This is required before the Install button will work (validation requires 16+ char keys).
 */
export const fillRequiredSecrets = async (page: Page) => {
  // Navigate to Security step
  await page.click('[data-step="2"]');
  // Click the Generate All Secrets button
  await page.click('button:has-text("Generate All Secrets")');
};
