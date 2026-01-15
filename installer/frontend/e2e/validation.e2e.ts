import { test, expect } from '@playwright/test';

test.describe('Form Validation', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('/');
  });

  test('shows validation error for empty hostname', async ({ page }) => {
    const input = page.locator('input#web-interface-hostname');
    await input.clear();
    await page.click('text=Next');

    // Should stay on domain step with error
    await expect(page.locator('[id$="-error"][role="alert"]')).toBeVisible();
    await expect(page.getByRole('heading', { name: 'Domain Configuration' })).toBeVisible();
  });

  test('accepts valid hostname', async ({ page }) => {
    const input = page.locator('input#web-interface-hostname');
    await input.fill('myzone.example.com');
    await page.click('text=Next');

    await expect(page.getByRole('heading', { name: 'Security' })).toBeVisible();
  });

  test('shows error for short security keys', async ({ page }) => {
    // Go to security step via step pill
    await page.click('[data-step="2"]');

    const masterKeyInput = page.locator('input#litellm-master-key');
    await masterKeyInput.fill('short');
    await page.click('text=Next');

    await expect(page.locator('[id$="-error"][role="alert"]').first()).toBeVisible();
    await expect(page.getByRole('heading', { name: 'Security' })).toBeVisible();
  });

  test('validates email format in advanced step', async ({ page }) => {
    // Navigate to advanced step via step pill
    await page.click('[data-step="7"]');

    const emailInput = page.getByLabel("ACME Email (for Let's Encrypt)");
    await emailInput.clear();
    await emailInput.fill('not-an-email');

    // Click Install - validation should prevent install and show error
    await page.click('button:has-text("Install")');

    // Wait for error to render (may need short delay for React state update)
    await page.waitForTimeout(100);

    // Check for validation error - use getByText as it's more specific
    await expect(page.getByText('Invalid email address')).toBeVisible({ timeout: 5000 });
    // Verify we're still on the Advanced step (not in modal)
    await expect(page.getByRole('heading', { name: 'Advanced Settings' })).toBeVisible();
  });
});
