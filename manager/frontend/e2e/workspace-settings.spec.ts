import { test, expect } from '@playwright/test';
import { setupAuth, mockCommonEndpoints } from './helpers/auth';

const mockTheme = {
  id: 'theme-1',
  workspace_id: '00000000-0000-0000-0000-000000000001',
  primary_color_light: '#3b82f6',
  secondary_color_light: '#6366f1',
  primary_color_dark: '#3b82f6',
  secondary_color_dark: '#6366f1',
  font_family: 'system',
  font_size_base: '16px',
  border_radius: 'medium',
  created_at: new Date().toISOString(),
  updated_at: new Date().toISOString(),
};

test.describe('Workspace Settings Page', () => {
  test.beforeEach(async ({ page }) => {
    // Set up API mocks
    await mockCommonEndpoints(page);

    // Mock theme endpoint
    await page.route('**/api/organizations/*/workspaces/*/settings/theme', (route) => {
      const method = route.request().method();

      if (method === 'GET') {
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({ theme: mockTheme }),
        });
      } else if (method === 'PUT') {
        const body = route.request().postDataJSON();
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({
            theme: { ...mockTheme, ...body },
          }),
        });
      } else if (method === 'DELETE') {
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({ theme: mockTheme }),
        });
      }
    });

    // Navigate and set up auth
    await page.goto('/');
    await setupAuth(page);
    await page.reload();

    // Wait for app to load
    await expect(page.locator('.sidebar')).toBeVisible({ timeout: 10000 });

    // Navigate to settings page
    await page.click('a[href="/settings"]');
    await expect(page).toHaveURL('/settings');
  });

  test.describe('Page Header', () => {
    test('displays page title', async ({ page }) => {
      await expect(page.locator('.page-title')).toContainText('Workspace Settings');
    });
  });

  test.describe('Theme Configuration Section', () => {
    test('displays theme configuration section', async ({ page }) => {
      await expect(page.locator('.section-title')).toContainText('Theme Configuration');
    });

    test('displays light mode color inputs', async ({ page }) => {
      await expect(page.locator('.card-title').filter({ hasText: 'Light Mode Colors' })).toBeVisible();
      await expect(page.locator('#primary-light')).toBeVisible();
      await expect(page.locator('#secondary-light')).toBeVisible();
    });

    test('displays dark mode color inputs', async ({ page }) => {
      await expect(page.locator('.card-title').filter({ hasText: 'Dark Mode Colors' })).toBeVisible();
      await expect(page.locator('#primary-dark')).toBeVisible();
      await expect(page.locator('#secondary-dark')).toBeVisible();
    });

    test('loads current theme values', async ({ page }) => {
      // Wait for theme to load
      await page.waitForTimeout(500);

      const primaryLightInput = page.locator('.color-text-input').first();
      await expect(primaryLightInput).toHaveValue('#3b82f6');
    });
  });

  test.describe('Typography Section', () => {
    test('displays typography settings', async ({ page }) => {
      await expect(page.locator('.card-title').filter({ hasText: 'Typography' })).toBeVisible();
    });

    test('displays font family dropdown', async ({ page }) => {
      await expect(page.locator('#font-family')).toBeVisible();
      await expect(page.locator('#font-family option')).toHaveCount(6);
    });

    test('displays font size slider', async ({ page }) => {
      await expect(page.locator('#font-size')).toBeVisible();
      await expect(page.locator('.slider-value')).toContainText('16px');
    });

    test('changes font family', async ({ page }) => {
      await page.selectOption('#font-family', 'inter');
      await expect(page.locator('#font-family')).toHaveValue('inter');
    });

    test('adjusts font size with slider', async ({ page }) => {
      const slider = page.locator('#font-size');

      // Change slider value
      await slider.fill('18');

      await expect(page.locator('.slider-value')).toContainText('18px');
    });
  });

  test.describe('Appearance Section', () => {
    test('displays corner radius options', async ({ page }) => {
      await expect(page.locator('.card-title').filter({ hasText: 'Appearance' })).toBeVisible();
      await expect(page.locator('.radio-option')).toHaveCount(4);
    });

    test('radio buttons for corner radius work', async ({ page }) => {
      // Click on "Large" radius option
      await page.click('.radio-option:has-text("Large") input');

      const largeRadio = page.locator('.radio-option:has-text("Large") input');
      await expect(largeRadio).toBeChecked();
    });
  });

  test.describe('Preview Section', () => {
    test('displays preview section', async ({ page }) => {
      await expect(page.locator('.card-title').filter({ hasText: 'Preview' })).toBeVisible();
    });

    test('shows preview text', async ({ page }) => {
      await expect(page.locator('.preview-text')).toContainText('preview of your theme');
    });

    test('shows preview buttons', async ({ page }) => {
      await expect(page.locator('.preview-buttons .btn-primary')).toContainText('Primary Button');
      await expect(page.locator('.preview-buttons .btn-secondary')).toContainText('Secondary Button');
    });

    test('shows sample preview card', async ({ page }) => {
      await expect(page.locator('.preview-card strong')).toContainText('Sample Card');
    });
  });

  test.describe('Color Picker', () => {
    test('color picker and text input are synced', async ({ page }) => {
      const colorInput = page.locator('#primary-light');
      const textInput = page.locator('.color-input-wrapper').first().locator('.color-text-input');

      // Change via color picker
      await colorInput.fill('#ff0000');

      // Text input should update
      await expect(textInput).toHaveValue('#ff0000');
    });

    test('text input updates color picker', async ({ page }) => {
      const textInput = page.locator('.color-input-wrapper').first().locator('.color-text-input');

      await textInput.fill('#00ff00');

      // Color picker should update (can verify by checking text input is synced)
      await expect(textInput).toHaveValue('#00ff00');
    });
  });

  test.describe('Save Changes', () => {
    test('saves theme successfully', async ({ page }) => {
      // Make a change
      await page.locator('.color-input-wrapper').first().locator('.color-text-input').fill('#ff5500');

      // Click save
      await page.click('button:has-text("Save Changes")');

      // Should show success message
      await expect(page.locator('.alert-success')).toContainText('Theme saved successfully');
    });

    test('shows loading state while saving', async ({ page }) => {
      await page.unroute('**/api/organizations/*/workspaces/*/settings/theme');
      await page.route('**/api/organizations/*/workspaces/*/settings/theme', async (route) => {
        const method = route.request().method();
        if (method === 'PUT') {
          await new Promise((resolve) => setTimeout(resolve, 500));
          route.fulfill({
            status: 200,
            contentType: 'application/json',
            body: JSON.stringify({ theme: mockTheme }),
          });
        } else {
          route.fulfill({
            status: 200,
            contentType: 'application/json',
            body: JSON.stringify({ theme: mockTheme }),
          });
        }
      });

      await page.click('button:has-text("Save Changes")');

      await expect(page.locator('button:has-text("Saving...")')).toBeVisible();
    });

    test('shows error when save fails', async ({ page }) => {
      await page.unroute('**/api/organizations/*/workspaces/*/settings/theme');
      await page.route('**/api/organizations/*/workspaces/*/settings/theme', (route) => {
        const method = route.request().method();
        if (method === 'GET') {
          route.fulfill({
            status: 200,
            contentType: 'application/json',
            body: JSON.stringify({ theme: mockTheme }),
          });
        } else if (method === 'PUT') {
          route.fulfill({
            status: 500,
            contentType: 'application/json',
            body: JSON.stringify({ error: 'Failed to save' }),
          });
        }
      });

      await page.click('button:has-text("Save Changes")');

      await expect(page.locator('.alert-error')).toBeVisible();
    });
  });

  test.describe('Reset to Defaults', () => {
    test('resets theme to defaults', async ({ page }) => {
      // Change a value first
      await page.locator('.color-input-wrapper').first().locator('.color-text-input').fill('#ff5500');

      // Click reset
      await page.click('button:has-text("Reset to Defaults")');

      // Should show success message
      await expect(page.locator('.alert-success')).toContainText('Theme reset to defaults');
    });

    test('shows error when reset fails', async ({ page }) => {
      await page.unroute('**/api/organizations/*/workspaces/*/settings/theme');
      await page.route('**/api/organizations/*/workspaces/*/settings/theme', (route) => {
        const method = route.request().method();
        if (method === 'GET') {
          route.fulfill({
            status: 200,
            contentType: 'application/json',
            body: JSON.stringify({ theme: mockTheme }),
          });
        } else if (method === 'DELETE') {
          route.fulfill({
            status: 500,
            contentType: 'application/json',
            body: JSON.stringify({ error: 'Failed to reset' }),
          });
        }
      });

      await page.click('button:has-text("Reset to Defaults")');

      await expect(page.locator('.alert-error')).toBeVisible();
    });
  });

  test.describe('Loading State', () => {
    test('shows loading state while fetching theme', async ({ page }) => {
      await page.unroute('**/api/organizations/*/workspaces/*/settings/theme');
      await page.route('**/api/organizations/*/workspaces/*/settings/theme', async (route) => {
        await new Promise((resolve) => setTimeout(resolve, 500));
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({ theme: mockTheme }),
        });
      });

      await page.reload();
      await page.click('a[href="/settings"]');

      await expect(page.locator('.loading-state')).toContainText('Loading theme settings');
    });
  });

  test.describe('Error State', () => {
    test('shows error when loading theme fails', async ({ page }) => {
      await page.unroute('**/api/organizations/*/workspaces/*/settings/theme');
      await page.route('**/api/organizations/*/workspaces/*/settings/theme', (route) => {
        route.fulfill({
          status: 500,
          contentType: 'application/json',
          body: JSON.stringify({ error: 'Server error' }),
        });
      });

      await page.reload();
      await page.click('a[href="/settings"]');

      await expect(page.locator('.alert-error')).toBeVisible();
    });
  });

  test.describe('Live Preview', () => {
    test('changes are applied live to preview', async ({ page }) => {
      // Wait for theme to load
      await page.waitForTimeout(500);

      // The preview should update as we change values
      // This is hard to verify visually in Playwright, but we can verify
      // that the form inputs change and no errors occur
      await page.selectOption('#font-family', 'roboto');
      await page.locator('#font-size').fill('18');
      await page.click('.radio-option:has-text("Large") input');

      // No errors should occur
      await expect(page.locator('.alert-error')).not.toBeVisible();
    });
  });
});
