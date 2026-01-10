import { test, expect } from '@playwright/test';

test.describe('Security Step Extended', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('/');
    // Navigate to Security step (step 2)
    await page.click('.stepper-item:nth-child(2) .stepper-button');
    await expect(page.locator('h2')).toContainText('Security');
  });

  test.describe('Authentication Fields', () => {
    test('displays authentication realm input', async ({ page }) => {
      await expect(page.getByLabel('Authentication Realm')).toBeVisible();
    });

    test('can change authentication realm', async ({ page }) => {
      const input = page.getByLabel('Authentication Realm');
      await input.clear();
      await input.fill('My Zone');
      await expect(input).toHaveValue('My Zone');
    });

    test('displays all security key inputs', async ({ page }) => {
      await expect(page.getByLabel('LiteLLM Master Key')).toBeVisible();
      await expect(page.getByLabel('LiteLLM Salt Key')).toBeVisible();
      await expect(page.getByLabel('SearXNG Secret Key')).toBeVisible();
      await expect(page.getByLabel('Manager API Key')).toBeVisible();
      await expect(page.getByLabel('PostgreSQL Password')).toBeVisible();
    });

    test('each key has a generate button', async ({ page }) => {
      // There should be multiple generate buttons (one for each key field)
      const generateButtons = page.locator('button:has-text("Generate")');
      // 5 individual + 1 "Generate All" = 6 total
      await expect(generateButtons).toHaveCount(6);
    });

    test('can generate individual Manager API Key', async ({ page }) => {
      const input = page.getByLabel('Manager API Key');
      const initialValue = await input.inputValue();

      // Find the generate button associated with Manager API Key
      const generateButton = input.locator('..').locator('button:has-text("Generate")');
      await generateButton.click();

      const newValue = await input.inputValue();
      expect(newValue).not.toBe(initialValue);
      expect(newValue.length).toBeGreaterThan(20);
    });

    test('can generate PostgreSQL password', async ({ page }) => {
      const input = page.getByLabel('PostgreSQL Password');
      const initialValue = await input.inputValue();

      const generateButton = input.locator('..').locator('button:has-text("Generate")');
      await generateButton.click();

      const newValue = await input.inputValue();
      expect(newValue).not.toBe(initialValue);
      expect(newValue.length).toBeGreaterThan(20);
    });

    test('Generate All creates all 5 keys', async ({ page }) => {
      // Clear all keys first
      const masterKey = page.getByLabel('LiteLLM Master Key');
      const saltKey = page.getByLabel('LiteLLM Salt Key');
      const searchKey = page.getByLabel('SearXNG Secret Key');
      const managerKey = page.getByLabel('Manager API Key');
      const postgresKey = page.getByLabel('PostgreSQL Password');

      await masterKey.clear();
      await saltKey.clear();
      await searchKey.clear();
      await managerKey.clear();
      await postgresKey.clear();

      // Generate all
      await page.click('button:has-text("Generate All Secrets")');

      // Verify all keys are populated
      expect((await masterKey.inputValue()).length).toBeGreaterThan(20);
      expect((await saltKey.inputValue()).length).toBeGreaterThan(20);
      expect((await searchKey.inputValue()).length).toBeGreaterThan(20);
      expect((await managerKey.inputValue()).length).toBeGreaterThan(20);
      expect((await postgresKey.inputValue()).length).toBeGreaterThan(20);
    });
  });

  test.describe('Production Settings', () => {
    test('displays HTTPS redirect checkbox', async ({ page }) => {
      await expect(page.getByLabel('Enable HTTPS redirect')).toBeVisible();
    });

    test('can toggle HTTPS redirect', async ({ page }) => {
      const checkbox = page.getByLabel('Enable HTTPS redirect');
      const initialState = await checkbox.isChecked();

      await checkbox.click();
      await expect(checkbox).toBeChecked({ checked: !initialState });
    });

    test('shows help text for HTTPS redirect', async ({ page }) => {
      await expect(page.getByText('Redirect HTTP to HTTPS')).toBeVisible();
    });

    test('displays TLS certificate checkbox', async ({ page }) => {
      await expect(page.getByLabel("Auto-generate TLS certificate (Let's Encrypt)")).toBeVisible();
    });

    test('can toggle TLS certificate generation', async ({ page }) => {
      const checkbox = page.getByLabel("Auto-generate TLS certificate (Let's Encrypt)");
      const initialState = await checkbox.isChecked();

      await checkbox.click();
      await expect(checkbox).toBeChecked({ checked: !initialState });
    });

    test('shows ACME info when TLS certificate enabled', async ({ page }) => {
      const checkbox = page.getByLabel("Auto-generate TLS certificate (Let's Encrypt)");

      if (!(await checkbox.isChecked())) {
        await checkbox.check();
      }

      await expect(page.getByText('Set your ACME email in Advanced settings')).toBeVisible();
    });

    test('hides ACME info when TLS certificate disabled', async ({ page }) => {
      const checkbox = page.getByLabel("Auto-generate TLS certificate (Let's Encrypt)");

      if (await checkbox.isChecked()) {
        await checkbox.uncheck();
      }

      await expect(page.getByText('Set your ACME email in Advanced settings')).not.toBeVisible();
    });
  });

  test.describe('Security Warnings', () => {
    test('shows warning about empty keys', async ({ page }) => {
      await expect(page.getByText('Empty keys are insecure')).toBeVisible();
    });
  });

  test.describe('Input Validation', () => {
    test('short master key shows validation error on next', async ({ page }) => {
      const masterKey = page.getByLabel('LiteLLM Master Key');
      await masterKey.clear();
      await masterKey.fill('short');

      await page.click('text=Next');

      await expect(page.locator('.ui-form-field__error').first()).toBeVisible();
      await expect(page.locator('h2')).toContainText('Security'); // Still on security step
    });

    test('valid keys allow navigation to next step', async ({ page }) => {
      // Generate all keys to ensure they're valid
      await page.click('button:has-text("Generate All Secrets")');

      await page.click('text=Next');

      // Should have navigated to the next step (Models)
      await expect(page.locator('h2')).toContainText('AI Provider Configuration');
    });
  });
});
