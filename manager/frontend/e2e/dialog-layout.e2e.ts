import type { Page, TestInfo } from '@playwright/test';
import { expect, test } from './fixtures';
import { setupAdminAuth, setupCommonRoutes } from './layout-fixtures';
import { blockServiceWorker } from './test-utils';

async function prepare(page: Page): Promise<void> {
  await blockServiceWorker(page.context());
  await setupCommonRoutes(page, true);
  await page.goto('/login', { waitUntil: 'domcontentloaded' });
  await setupAdminAuth(page);
}

async function capture(page: Page, information: TestInfo): Promise<void> {
  await page.evaluate(() => document.fonts.ready);
  await page.screenshot({
    path: information.outputPath('dialog.png'),
    animations: 'disabled',
  });
}

for (const width of [1440, 390]) {
  for (const motion of ['no-preference', 'reduce'] as const) {
    test.describe(`Dialog layout ${width}px ${motion}`, () => {
      test.use({ viewport: { width, height: 1000 }, reducedMotion: motion });
      test.beforeEach(async ({ page }) => {
        await page.emulateMedia({ reducedMotion: motion });
        await prepare(page);
        expect(
          await page.evaluate(() => matchMedia('(prefers-reduced-motion: reduce)').matches)
        ).toBe(motion === 'reduce');
      });

      test('wizard preserves its content and closes when the backdrop is clicked', async ({
        page,
      }, information) => {
        await page.goto('/projects', { waitUntil: 'domcontentloaded' });
        await page.getByRole('button', { name: '+ New Project', exact: true }).click();
        const dialog = page.getByRole('dialog');
        await expect(dialog).toBeVisible();
        await expect(
          dialog.getByRole('heading', { name: 'New Project', exact: true })
        ).toBeVisible();
        await expect(dialog.getByRole('navigation', { name: 'Wizard steps' })).toBeVisible();
        await expect(dialog.getByLabel('Project Name')).toBeVisible();
        await expect(dialog.getByRole('button', { name: 'Cancel', exact: true })).toBeVisible();
        await expect(dialog.getByRole('button', { name: 'Next', exact: true })).toBeVisible();
        await dialog.getByLabel('Project Name').fill('Keep the wizard open');
        await expect(dialog).toBeVisible();
        await capture(page, information);
        await page.mouse.click(8, 8);
        await expect(dialog).toBeHidden();
        await page.getByRole('button', { name: '+ New Project', exact: true }).click();
        await expect(dialog).toBeVisible();
        await expect(page.locator('.ui-wizard-overlay')).toHaveCSS('position', 'fixed');
        await expect(dialog.locator('.ui-wizard-content')).toHaveCSS(
          'padding-left',
          width <= 768 ? '16px' : '24px'
        );
        if (motion === 'reduce') {
          await expect(dialog).toHaveCSS('animation-name', 'none');
          await expect(page.locator('.ui-wizard-overlay')).toHaveCSS('animation-name', 'none');
        }
        await page.keyboard.press('Escape');
        await expect(dialog).toBeHidden();
      });

      test('confirmation dialog keeps its fixed panel and spacing', async ({
        page,
      }, information) => {
        await page.goto('/sessions', { waitUntil: 'domcontentloaded' });
        await page.getByRole('button', { name: 'Revoke All Other Sessions' }).click();
        const dialog = page.getByRole('dialog');
        await expect(dialog).toBeVisible();
        await expect(dialog).toHaveCSS('position', 'fixed');
        await expect(dialog).toHaveCSS('padding', width <= 768 ? '16px' : '24px');
        await expect(dialog).toHaveCSS('border-top-width', '1px');
        await expect(dialog).not.toHaveCSS('background-color', 'rgba(0, 0, 0, 0)');
        await expect(page.locator('.ui-dialog-overlay')).toHaveCSS('position', 'fixed');
        await expect(
          dialog.getByRole('heading', { name: 'Revoke All Other Sessions' })
        ).toBeVisible();
        await expect(dialog.locator('p')).toContainText('Are you sure you want to revoke');
        await expect(dialog.getByRole('button', { name: 'Cancel', exact: true })).toBeVisible();
        await expect(dialog.getByRole('button', { name: 'Confirm', exact: true })).toBeVisible();
        if (motion === 'reduce') {
          await expect(dialog).toHaveCSS('animation-name', 'none');
          await expect(page.locator('.ui-dialog-overlay')).toHaveCSS('animation-name', 'none');
        }
        await capture(page, information);
        await dialog.getByRole('button', { name: 'Cancel', exact: true }).click();
        await expect(dialog).toBeHidden();
      });
    });
  }
}
