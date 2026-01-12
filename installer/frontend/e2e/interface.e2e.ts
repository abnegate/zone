import { test, expect } from '@playwright/test';

test.describe('Interface Settings', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('/');
    // Navigate to Interface step (step 4)
    await page.click('.stepper-item:nth-child(4) .stepper-button');
    await expect(page.locator('h2')).toContainText('Interface Settings');
  });

  test('displays authentication checkbox', async ({ page }) => {
    await expect(page.getByLabel('Enable built-in authentication')).toBeVisible();
  });

  test('can toggle authentication', async ({ page }) => {
    const checkbox = page.getByLabel('Enable built-in authentication');

    const initialState = await checkbox.isChecked();

    await checkbox.click();
    await expect(checkbox).toBeChecked({ checked: !initialState });

    await checkbox.click();
    await expect(checkbox).toBeChecked({ checked: initialState });
  });

  test('displays signup checkbox', async ({ page }) => {
    await expect(page.getByLabel('Allow user signups')).toBeVisible();
  });

  test('can toggle user signups', async ({ page }) => {
    const checkbox = page.getByLabel('Allow user signups');

    const initialState = await checkbox.isChecked();

    await checkbox.click();
    await expect(checkbox).toBeChecked({ checked: !initialState });
  });

  test('displays language selector', async ({ page }) => {
    await expect(page.getByLabel('Default Language')).toBeVisible();
  });

  test('can select different languages', async ({ page }) => {
    const langSelect = page.getByLabel('Default Language');

    const languages = ['en-US', 'en-GB', 'es-ES', 'fr-FR', 'de-DE', 'ja-JP', 'zh-CN'];

    for (const lang of languages) {
      await langSelect.selectOption(lang);
      await expect(langSelect).toHaveValue(lang);
    }
  });

  test('shows help text for authentication', async ({ page }) => {
    await expect(page.getByText('Uses Traefik basic auth by default')).toBeVisible();
  });

  test('authentication and signup are independent', async ({ page }) => {
    const authCheckbox = page.getByLabel('Enable built-in authentication');
    const signupCheckbox = page.getByLabel('Allow user signups');

    // Enable both
    await authCheckbox.check();
    await signupCheckbox.check();

    await expect(authCheckbox).toBeChecked();
    await expect(signupCheckbox).toBeChecked();

    // Disable auth only
    await authCheckbox.uncheck();

    await expect(authCheckbox).not.toBeChecked();
    await expect(signupCheckbox).toBeChecked();
  });
});
