import { test, expect } from '@playwright/test';

test.describe('Secret Generation', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('/');
    // Navigate to security step via step pill
    await page.click('.stepper-item:nth-child(2) .stepper-button');
  });

  test('generates secret on button click', async ({ page }) => {
    const input = page.locator('input#litellm-master-key');
    const initialValue = await input.inputValue();

    await page.locator('button:has-text("Generate")').first().click();

    const newValue = await input.inputValue();
    expect(newValue).not.toBe(initialValue);
    expect(newValue.length).toBeGreaterThan(20);
  });

  test('generates all secrets', async ({ page }) => {
    await page.click('button:has-text("Generate All Secrets")');

    const masterKey = await page.locator('input#litellm-master-key').inputValue();
    const saltKey = await page.locator('input#litellm-salt-key').inputValue();
    const searchKey = await page.locator('input#searxng-secret-key').inputValue();

    expect(masterKey.length).toBeGreaterThan(20);
    expect(saltKey.length).toBeGreaterThan(20);
    expect(searchKey.length).toBeGreaterThan(20);
  });

  test('each generated secret is unique', async ({ page }) => {
    await page.click('button:has-text("Generate All Secrets")');

    const masterKey = await page.locator('input#litellm-master-key').inputValue();
    const saltKey = await page.locator('input#litellm-salt-key').inputValue();
    const searchKey = await page.locator('input#searxng-secret-key').inputValue();

    expect(masterKey).not.toBe(saltKey);
    expect(saltKey).not.toBe(searchKey);
    expect(masterKey).not.toBe(searchKey);
  });
});
